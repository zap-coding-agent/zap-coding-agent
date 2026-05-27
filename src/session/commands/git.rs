use std::sync::atomic::Ordering;
use colored::Colorize;
use anyhow::Result;
use crate::llm_client::{BeforeOutput, ContentBlock, Message};
use super::super::Session;

impl Session {
    pub async fn cmd_branch(&mut self, name: &str) {
        if name.is_empty() { println!("  usage: /branch <name>"); return; }
        let json = match serde_json::to_string(&self.messages) {
            Ok(j) => j, Err(e) => { println!("  {} {}", "✗".red(), e); return; }
        };
        match self.store.save_branch(self.session_id, name, &self.current_branch, &json, self.turn_count) {
            Ok(_) => {
                let old = self.current_branch.clone();
                self.current_branch = name.to_string();
                println!("  {} branched from {} → {}", "✓".green(), old.dimmed(), name.cyan().bold());
                println!("  {} conversation forked — changes stay on '{}' until you /switch", "·".dimmed(), name.cyan());
            }
            Err(e) => println!("  {} {}", "✗".red(), e),
        }
    }

    pub fn cmd_branches(&self) {
        match self.store.list_branches(self.session_id) {
            Ok(branches) if branches.is_empty() => {
                println!("  No branches (only main). Create one with /branch <name>");
            }
            Ok(branches) => {
                println!();
                for (name, parent, turns, _) in &branches {
                    let marker = if name == &self.current_branch { " ← current".green().to_string() } else { String::new() };
                    println!("  {}  {}  {} turns  from: {}{}", "·".dimmed(), name.cyan().bold(), turns, parent.dimmed(), marker);
                }
                println!();
            }
            Err(e) => println!("  {} {}", "✗".red(), e),
        }
    }

    pub async fn cmd_switch(&mut self, name: &str) {
        if name.is_empty() { println!("  usage: /switch <branch-name>"); return; }
        let target = name.to_string();

        if let Ok(json) = serde_json::to_string(&self.messages) {
            let _ = self.store.save_branch(self.session_id, &self.current_branch, "main", &json, self.turn_count);
        }

        if target == "main" {
            match self.store.load_messages(self.session_id) {
                Ok(Some(json)) => {
                    if let Ok(msgs) = serde_json::from_str(&json) {
                        let old = self.current_branch.clone();
                        self.messages       = msgs;
                        self.turn_count     = self.messages.iter().filter(|m| m.role == "user").count();
                        self.current_branch = "main".to_string();
                        println!("  {} switched {} → main", "✓".green(), old.dimmed());
                    }
                }
                _ => println!("  {} main branch state not found", "✗".red()),
            }
        } else {
            match self.store.load_branch(self.session_id, &target) {
                Ok(Some((json, turns))) => {
                    if let Ok(msgs) = serde_json::from_str(&json) {
                        let old = self.current_branch.clone();
                        self.messages       = msgs;
                        self.turn_count     = turns;
                        self.current_branch = target.clone();
                        println!("  {} switched {} → {}", "✓".green(), old.dimmed(), target.cyan().bold());
                    }
                }
                Ok(None) => println!("  {} branch '{}' not found", "✗".red(), target),
                Err(e)   => println!("  {} {}", "✗".red(), e),
            }
        }
    }

    pub async fn cmd_merge(&mut self, name: &str) {
        if name.is_empty() { println!("  usage: /merge <branch-name>"); return; }

        let branch_msgs: Vec<Message> = match self.store.load_branch(self.session_id, name) {
            Ok(Some((json, _))) => match serde_json::from_str(&json) {
                Ok(m) => m,
                Err(e) => { println!("  {} could not parse branch: {}", "✗".red(), e); return; }
            },
            Ok(None) => { println!("  {} branch '{}' not found", "✗".red(), name); return; }
            Err(e)   => { println!("  {} {}", "✗".red(), e); return; }
        };

        let branch_text: String = branch_msgs.iter()
            .filter_map(|m| {
                let t: String = m.content.iter().filter_map(|b| {
                    if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None }
                }).collect::<Vec<_>>().join(" ");
                if t.trim().is_empty() { None } else { Some(format!("[{}] {}", m.role, t.trim())) }
            }).collect::<Vec<_>>().join("\n");

        let prompt = format!("Summarize this conversation branch in 3 sentences, focusing on conclusions and decisions made:\n\n{}", branch_text);

        println!("  {} summarizing branch '{}'…", "◌".dimmed(), name);
        let mut spinner = Self::make_spinner();
        let pb_clone   = spinner.pb_clone();
        let stop_clone = spinner.stop_signal();
        let before: BeforeOutput = Box::new(move || {
            stop_clone.store(true, Ordering::Relaxed);
            pb_clone.finish_and_clear();
        });
        let resp = self.client.send(
            "You summarize conversations concisely.",
            &[Message::user_text(&prompt)], &[], Some(before), 0,
        ).await;
        spinner.finish_and_clear();

        match resp {
            Ok(r) => {
                let summary = r.content.iter()
                    .filter_map(|b| if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None })
                    .collect::<Vec<_>>().join("\n");
                let merge_msg = format!("[merged from branch '{}']\n{}", name, summary);
                self.messages.push(Message {
                    role:    "assistant".to_string(),
                    content: vec![ContentBlock::Text { text: merge_msg }],
                });
                println!("  {} merged '{}' into '{}'", "✓".green(), name.cyan(), self.current_branch.cyan().bold());
                println!("  {}", summary.dimmed());
            }
            Err(e) => println!("  {} merge summary failed: {}", "✗".red(), e),
        }
    }

    /// Run `scripts/deploy.sh` with live streaming output — no LLM involved.
    pub async fn cmd_deploy(&self, arg: &str) {
        use tokio::io::AsyncBufReadExt;

        let script = "scripts/deploy.sh";
        if !std::path::Path::new(script).exists() {
            println!("  {} {} not found", "✗".red(), script.cyan());
            return;
        }

        let args: Vec<&str> = if arg.is_empty() { vec![] } else { arg.split_whitespace().collect() };
        let label = if args.contains(&"--check") { "deploy --check" } else { "deploy" };

        println!();
        println!("  {} {}", "⚡".bright_yellow(), label.bold());
        println!("  {}", "─".repeat(44).truecolor(60, 55, 80));

        let mut child = match tokio::process::Command::new("bash")
            .arg(script)
            .args(&args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(c) => c,
            Err(e) => { println!("  {} failed to start: {}", "✗".red(), e); return; }
        };

        let stdout = child.stdout.take().map(tokio::io::BufReader::new);
        let stderr = child.stderr.take().map(tokio::io::BufReader::new);

        let print_line = |line: &str| {
            println!("  {}", line);
        };

        if let (Some(out), Some(err)) = (stdout, stderr) {
            let mut out_lines = out.lines();
            let mut err_lines = err.lines();
            loop {
                tokio::select! {
                    line = out_lines.next_line() => match line {
                        Ok(Some(l)) => print_line(&l),
                        Ok(None) => break,
                        Err(_) => break,
                    },
                    line = err_lines.next_line() => match line {
                        Ok(Some(l)) => print_line(&l),
                        Ok(None) => {},
                        Err(_) => {},
                    },
                }
            }
            while let Ok(Some(l)) = err_lines.next_line().await {
                print_line(&l);
            }
        }

        match child.wait().await {
            Ok(status) if status.success() => {
                println!("  {}", "─".repeat(44).truecolor(60, 55, 80));
                println!("  {} done", "✓".green());
            }
            Ok(status) => {
                println!("  {}", "─".repeat(44).truecolor(60, 55, 80));
                println!("  {} exited with status {}", "✗".red(), status.code().unwrap_or(-1));
            }
            Err(e) => println!("  {} wait error: {}", "✗".red(), e),
        }
        println!();
    }

    pub async fn cmd_run_workflow(&mut self, name: &str) -> Result<()> {
        let workflow = crate::workflow::load_workflow(name)?;
        println!();
        println!("  {} {} — {}",
            "⚡".bright_yellow(),
            format!("workflow: {}", workflow.name).bold(),
            workflow.description.dimmed());
        println!("  {} {} step(s)", "◌".dimmed(), workflow.steps.len());
        println!();

        for (i, step) in workflow.steps.iter().enumerate() {
            println!("  {} step {}/{}{}",
                "▶".cyan(), i + 1, workflow.steps.len(),
                if step.skill.is_empty() { String::new() }
                else { format!("  [skill: {}]", step.skill).dimmed().to_string() });

            if step.requires_approval {
                use std::io::Write;
                print!("  Continue? [y/N] ");
                std::io::stdout().flush()?;
                let mut line = String::new();
                std::io::stdin().read_line(&mut line)?;
                if !matches!(line.trim().to_lowercase().as_str(), "y" | "yes") {
                    println!("  {} Stopped at step {}.", "✗".red(), i + 1);
                    return Ok(());
                }
            }

            let prompt = if step.skill.is_empty() { step.prompt.clone() }
                         else { format!("[Using skill: {}]\n{}", step.skill, step.prompt) };

            if let Err(e) = self.handle_user_turn(&prompt).await {
                println!("  {} step {} failed: {}", "✗".red(), i + 1, e);
                return Err(e);
            }
        }
        println!("  {} workflow '{}' complete.", "✓".green(), workflow.name.cyan());
        Ok(())
    }
}
