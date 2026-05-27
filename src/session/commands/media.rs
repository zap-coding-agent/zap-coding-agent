use colored::Colorize;
use super::super::Session;

impl Session {
    pub fn cmd_paste(&mut self) {
        if !crate::llm_client::provider_supports_vision(&self.config) {
            println!("  {} {} does not support vision — image will not be sent.",
                "✗".red(),
                self.config.base_url.as_deref().unwrap_or("this provider").cyan());
            println!("  {} Switch to Claude or GPT-4o to use images.", "·".dimmed());
            return;
        }

        #[cfg(windows)]
        let tmp = r"C:\Windows\Temp\zap_clipboard_paste.png";
        #[cfg(not(windows))]
        let tmp = "/tmp/zap_clipboard_paste.png";

        let ok = paste_clipboard_image(tmp);

        if ok && std::path::Path::new(tmp).exists() {
            self.cmd_attach(tmp);
        } else {
            println!("  {} No image in clipboard. Copy a screenshot first, then run /paste.", "✗".red());
            println!("  {} You can also use {} to stage a file directly.", "·".dimmed(), "/attach <path>".cyan());
        }
    }

    pub fn cmd_attach(&mut self, path: &str) {
        if !crate::llm_client::provider_supports_vision(&self.config) {
            println!("  {} {} does not support vision — image will not be sent.",
                "✗".red(),
                self.config.base_url.as_deref().unwrap_or("this provider").cyan());
            println!("  {} Switch to Claude or GPT-4o to use images.", "·".dimmed());
            return;
        }

        let path = path.trim();
        if path.is_empty() {
            println!("  Usage: /attach <image-path>");
            return;
        }
        let mime = match std::path::Path::new(path)
            .extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref()
        {
            Some("png")            => "image/png",
            Some("jpg") | Some("jpeg") => "image/jpeg",
            Some("gif")            => "image/gif",
            Some("webp")           => "image/webp",
            _ => {
                println!("  {} Unsupported format. Use png / jpg / gif / webp.", "✗".red());
                return;
            }
        };
        match std::fs::read(path) {
            Ok(bytes) => {
                use base64::Engine;
                let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
                let kb   = bytes.len() / 1024;
                println!("  {} Attached {} ({} KB, {})", "✓".green(), path.cyan(), kb, mime.dimmed());
                self.staged_images.push((mime.to_string(), data));
            }
            Err(e) => println!("  {} Could not read '{}': {}", "✗".red(), path, e),
        }
    }
}

/// Try every available method to write the clipboard image to `dest`.
/// Returns true if the file was written successfully.
#[allow(clippy::needless_return)]
pub fn paste_clipboard_image(dest: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        // Fast path: pngpaste CLI (brew install pngpaste)
        if std::process::Command::new("pngpaste")
            .arg(dest)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return true;
        }
        // Fallback: AppleScript — escape path to prevent script injection
        let safe_dest = dest.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!(
            r#"try
  set d to (the clipboard as «class PNGf»)
  set f to open for access POSIX file "{safe_dest}" with write permission
  set eof f to 0
  write d to f
  close access f
  return true
on error
  return false
end try"#
        );
        return std::process::Command::new("osascript")
            .args(["-e", &script])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
            .unwrap_or(false);
    }

    #[cfg(target_os = "windows")]
    {
        // Escape single quotes in path for PowerShell string context
        let safe_dest = dest.replace('\'', "''");
        let script = format!(
            r#"Add-Type -Assembly System.Windows.Forms; \
$img = [System.Windows.Forms.Clipboard]::GetImage(); \
if ($img -eq $null) {{ exit 1 }}; \
$img.Save('{safe_dest}'); exit 0"#
        );
        return std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let xclip_ok = std::process::Command::new("xclip")
            .args(["-selection", "clipboard", "-t", "image/png", "-o"])
            .output()
            .map(|o| {
                if o.status.success() && !o.stdout.is_empty() {
                    std::fs::write(dest, &o.stdout).is_ok()
                } else {
                    false
                }
            })
            .unwrap_or(false);
        if xclip_ok { return true; }

        std::process::Command::new("wl-paste")
            .args(["--type", "image/png"])
            .output()
            .map(|o| {
                if o.status.success() && !o.stdout.is_empty() {
                    std::fs::write(dest, &o.stdout).is_ok()
                } else {
                    false
                }
            })
            .unwrap_or(false)
    }
}
