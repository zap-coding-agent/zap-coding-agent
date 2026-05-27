use colored::Colorize;
use super::super::Session;

impl Session {
    pub async fn cmd_tasks(&mut self) {
        use inquire::Select;
        let cfg = crate::ui::inquire_render_config();

        let task_files = crate::task_planner::discover_task_files();

        if task_files.is_empty() {
            println!("  {} No task sessions found.", "✗".red());
            println!("  {} Start one by selecting {} mode at startup.", "·".dimmed(), "Task".cyan());
            println!("  {} Task files live in {}", "·".dimmed(), ".zap/tasks/<session>/tasks.md".cyan());
            return;
        }

        let folder_labels: Vec<String> = task_files.iter().map(|tf| {
            let done  = tf.done_count();
            let total = tf.tasks.len();
            let bar   = if total == 0 { String::new() } else {
                let filled = (done * 10) / total;
                format!("[{}{}]", "█".repeat(filled), "░".repeat(10 - filled))
            };
            format!("{:<40} {}/{} done  {}  {}",
                tf.folder,
                done, total,
                bar,
                tf.path.display(),
            )
        }).collect();

        let chosen_folder = match Select::new("Task session:", folder_labels.iter().map(|s| s.as_str()).collect())
            .with_render_config(cfg)
            .with_help_message("↑↓ navigate   Enter select   Esc cancel")
            .with_page_size(10)
            .prompt_skippable()
        {
            Ok(Some(s)) => s.to_string(),
            _ => return,
        };

        let tf_idx = match folder_labels.iter().position(|l| l == &chosen_folder) {
            Some(i) => i,
            None    => return,
        };
        let tf = &task_files[tf_idx];

        println!();
        println!(
            "  {} {}  {}/{}  {}",
            "◆".truecolor(255,210,50),
            tf.goal.truecolor(255,210,50).bold(),
            tf.done_count(), tf.tasks.len(),
            tf.path.display().to_string().truecolor(100,95,130),
        );
        println!("  {}", "─".repeat(56).truecolor(60,55,80));

        let task_labels: Vec<String> = tf.tasks.iter().map(|t| {
            let icon    = if t.is_done() { "✓".green().to_string() } else { "○".truecolor(150,145,170).to_string() };
            let skill   = t.skill_name.as_deref()
                .map(|s| format!("  [{}]", s))
                .unwrap_or_default();
            let done_steps = t.steps.iter().filter(|(c,_)| *c).count();
            let total_steps = t.steps.len();
            format!("{} {}. {}{}  {}/{}",
                icon,
                t.number,
                t.title,
                skill.truecolor(100,95,130),
                done_steps, total_steps,
            )
        }).collect();

        let mut options: Vec<&str> = task_labels.iter().map(|s| s.as_str()).collect();
        options.push("← back");

        let chosen_task = match Select::new("Select task to execute:", options)
            .with_render_config(cfg)
            .with_help_message("↑↓ navigate   Enter execute   Esc cancel")
            .with_page_size(12)
            .prompt_skippable()
        {
            Ok(Some(s)) if s != "← back" => s.to_string(),
            _ => return,
        };

        let task_idx = match task_labels.iter().position(|l| l == &chosen_task) {
            Some(i) => i,
            None    => return,
        };
        let task = &tf.tasks[task_idx];

        if task.is_done() {
            println!("  {} Task {} is already done. Re-run anyway? [y/N] ", "·".dimmed(), task.number);
            let mut ans = String::new();
            std::io::stdin().read_line(&mut ans).ok();
            if !ans.trim().eq_ignore_ascii_case("y") { return; }
        }

        println!();
        println!("  {} Executing task {}…", "▶".cyan().bold(), task.number);
        if let Err(e) = self.handle_user_turn(&task.execution_prompt()).await {
            println!("  {} {}", "✗".red(), e);
        }
    }
}
