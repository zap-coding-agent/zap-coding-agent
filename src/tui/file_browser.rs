/// File browser for the TUI - navigate and preview files.
use std::path::{Path, PathBuf};
use std::fs;
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub is_expanded: bool,
    pub depth: usize,
    pub git_status: GitStatus,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GitStatus {
    Untracked,
    Modified,
    Staged,
    Clean,
    Ignored,
}

pub struct FileBrowser {
    pub entries: Vec<FileEntry>,
    pub selected: usize,
    pub scroll: usize,
    pub search_query: String,
    pub root_path: PathBuf,
    pub preview_content: Option<String>,
    pub preview_lang: Option<String>,
}

impl FileBrowser {
    pub fn new(root_path: PathBuf) -> Result<Self> {
        let mut browser = Self {
            entries: Vec::new(),
            selected: 0,
            scroll: 0,
            search_query: String::new(),
            root_path: root_path.clone(),
            preview_content: None,
            preview_lang: None,
        };
        
        browser.load_entries()?;
        Ok(browser)
    }
    
    /// Load directory entries recursively.
    fn load_entries(&mut self) -> Result<()> {
        self.entries.clear();
        self.scan_directory(&self.root_path.clone(), 0)?;
        Ok(())
    }
    
    /// Recursively scan a directory.
    fn scan_directory(&mut self, path: &Path, depth: usize) -> Result<()> {
        if depth > 10 {
            return Ok(()); // Prevent infinite recursion
        }
        
        let mut entries: Vec<_> = fs::read_dir(path)?
            .filter_map(|e| e.ok())
            .collect();
        
        // Sort: directories first, then files, alphabetically
        entries.sort_by(|a, b| {
            let a_is_dir = a.path().is_dir();
            let b_is_dir = b.path().is_dir();
            
            match (a_is_dir, b_is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.file_name().cmp(&b.file_name()),
            }
        });
        
        for entry in entries {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            
            // Skip hidden files and common ignore patterns
            if name.starts_with('.') && name != ".gitignore" {
                continue;
            }
            if name == "node_modules" || name == "target" || name == "__pycache__" {
                continue;
            }
            
            let is_dir = path.is_dir();
            let git_status = self.get_git_status(&path);
            
            self.entries.push(FileEntry {
                path: path.clone(),
                name,
                is_dir,
                is_expanded: false,
                depth,
                git_status,
            });
            
            // Don't auto-expand directories initially
        }
        
        Ok(())
    }
    
    /// Get git status for a file.
    fn get_git_status(&self, path: &Path) -> GitStatus {
        // Simple implementation - check if file is in git status output
        let output = std::process::Command::new("git")
            .args(["status", "--porcelain", "--ignored"])
            .arg(path)
            .output();
        
        if let Ok(output) = output {
            if output.status.success() {
                let status_str = String::from_utf8_lossy(&output.stdout);
                if status_str.starts_with("??") {
                    return GitStatus::Untracked;
                } else if status_str.starts_with(" M") || status_str.starts_with("M ") {
                    return GitStatus::Modified;
                } else if status_str.starts_with("A ") {
                    return GitStatus::Staged;
                } else if status_str.starts_with("!!") {
                    return GitStatus::Ignored;
                }
            }
        }
        
        GitStatus::Clean
    }
    
    /// Toggle expansion of a directory.
    pub fn toggle_expand(&mut self) -> Result<()> {
        if self.selected >= self.entries.len() {
            return Ok(());
        }
        
        let entry = &self.entries[self.selected];
        if !entry.is_dir {
            return Ok(());
        }
        
        let is_expanded = entry.is_expanded;
        let path = entry.path.clone();
        let depth = entry.depth;
        
        if is_expanded {
            // Collapse: remove all children
            self.entries[self.selected].is_expanded = false;
            let i = self.selected + 1;
            while i < self.entries.len() && self.entries[i].depth > depth {
                self.entries.remove(i);
            }
        } else {
            // Expand: insert children
            self.entries[self.selected].is_expanded = true;
            let children = self.get_directory_children(&path, depth + 1)?;
            
            // Insert children after the current entry
            for (i, child) in children.into_iter().enumerate() {
                self.entries.insert(self.selected + 1 + i, child);
            }
        }
        
        Ok(())
    }
    
    /// Get children of a directory.
    fn get_directory_children(&self, path: &Path, depth: usize) -> Result<Vec<FileEntry>> {
        let mut children = Vec::new();
        
        let mut entries: Vec<_> = fs::read_dir(path)?
            .filter_map(|e| e.ok())
            .collect();
        
        entries.sort_by(|a, b| {
            let a_is_dir = a.path().is_dir();
            let b_is_dir = b.path().is_dir();
            
            match (a_is_dir, b_is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.file_name().cmp(&b.file_name()),
            }
        });
        
        for entry in entries {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            
            if name.starts_with('.') && name != ".gitignore" {
                continue;
            }
            if name == "node_modules" || name == "target" || name == "__pycache__" {
                continue;
            }
            
            let is_dir = path.is_dir();
            let git_status = self.get_git_status(&path);
            
            children.push(FileEntry {
                path,
                name,
                is_dir,
                is_expanded: false,
                depth,
                git_status,
            });
        }
        
        Ok(children)
    }
    
    /// Load preview for the selected file.
    pub fn load_preview(&mut self) -> Result<()> {
        if self.selected >= self.entries.len() {
            return Ok(());
        }
        
        let entry = &self.entries[self.selected];
        if entry.is_dir {
            self.preview_content = None;
            self.preview_lang = None;
            return Ok(());
        }
        
        // Read file content (limit to 10KB for preview)
        match fs::read_to_string(&entry.path) {
            Ok(content) => {
                let preview = if content.len() > 10_000 {
                    format!("{}...\n\n[File too large, showing first 10KB]", &content[..10_000])
                } else {
                    content
                };
                
                // Detect language from extension
                let lang = entry.path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_string());
                
                self.preview_content = Some(preview);
                self.preview_lang = lang;
            }
            Err(_) => {
                self.preview_content = Some("[Binary file or read error]".to_string());
                self.preview_lang = None;
            }
        }
        
        Ok(())
    }
    
    /// Move selection up.
    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }
    
    /// Move selection down.
    pub fn move_down(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
        }
    }
    
    /// Get filtered entries based on search query.
    pub fn filtered_entries(&self) -> Vec<(usize, &FileEntry)> {
        if self.search_query.is_empty() {
            return self.entries.iter().enumerate().collect();
        }
        
        let query = self.search_query.to_lowercase();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.name.to_lowercase().contains(&query))
            .collect()
    }
    
    /// Get the selected file path.
    pub fn selected_path(&self) -> Option<PathBuf> {
        self.entries.get(self.selected).map(|e| e.path.clone())
    }
}
