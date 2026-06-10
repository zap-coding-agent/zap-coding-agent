# 🗂️ File Browser Guide

## Quick Start

Press **Ctrl+F** to open the file browser!

## Features

### 📁 Tree View
- Hierarchical file and directory listing
- Collapsible folders
- Smart filtering (hides node_modules, target, __pycache__, etc.)
- Sorted: directories first, then files, alphabetically

### 👁️ Live Preview
- Syntax-highlighted code preview
- Updates automatically as you navigate
- Supports 50+ languages
- Shows first 10KB of large files

### 🎨 Git Status Indicators
- **M** - Modified (yellow)
- **?** - Untracked (red)
- **A** - Staged (green)
- **!** - Ignored (dark gray)
- **[space]** - Clean (gray)

### ⌨️ Keyboard Navigation
| Key | Action |
|-----|--------|
| **Ctrl+F** | Toggle file browser |
| **↑ / k** | Move up |
| **↓ / j** | Move down |
| **Enter / → / l** | Open file or expand directory |
| **← / h** | Collapse directory |
| **Esc / q** | Close browser |

## Usage Examples

### 1. Browse Project Files
```
1. Press Ctrl+F
2. Use ↑↓ to navigate
3. Press Enter on directories to expand
4. See preview on the right
```

### 2. Open a File for Discussion
```
1. Press Ctrl+F
2. Navigate to the file
3. Press Enter
4. File path is inserted into chat: "show me /path/to/file.rs"
5. Press Enter to send
```

### 3. Explore Directory Structure
```
1. Press Ctrl+F
2. Expand directories with Enter
3. Collapse with ← or h
4. Navigate the tree structure
```

## Visual Layout

```
┌─────────────────────────────────────────────────────────────┐
│                    Files (Ctrl+F to close)                  │
├──────────────────────┬──────────────────────────────────────┤
│ File List (40%)      │ Preview (60%)                        │
│                      │                                      │
│ ▼  src/             │ // Syntax-highlighted preview        │
│   M main.rs          │ fn main() {                          │
│   ? new_file.rs      │     println!("Hello!");              │
│ ▶  tests/            │ }                                    │
│   A README.md        │                                      │
│                      │                                      │
│ ↑↓ navigate          │                                      │
│ Enter open           │                                      │
│ Esc close            │                                      │
└──────────────────────┴──────────────────────────────────────┘
```

## Tips & Tricks

### 1. Vim-Style Navigation
If you're familiar with vim, use:
- **j** - down
- **k** - up
- **h** - collapse/left
- **l** - expand/right

### 2. Quick File Access
- Navigate to a file
- Press Enter
- The path is inserted into chat
- Just press Enter again to ask about it

### 3. Git Status at a Glance
- Yellow **M** - You've modified this file
- Red **?** - New file, not tracked
- Green **A** - Staged for commit
- Look for these indicators to see what's changed

### 4. Preview Before Opening
- Navigate through files
- Preview updates automatically
- See syntax-highlighted code
- Decide if it's the file you need

## Keyboard Shortcuts Summary

```
Ctrl+F  →  Toggle file browser
↑↓ j k  →  Navigate
Enter →  →  Open/Expand
← h     →  Collapse
Esc q   →  Close
```

## What Gets Filtered Out?

The browser automatically hides:
- Hidden files (starting with `.`) except `.gitignore`
- `node_modules/` - npm packages
- `target/` - Rust build artifacts
- `__pycache__/` - Python cache
- Other common build/cache directories

## Integration with Chat

When you press Enter on a file:
1. File path is inserted: `show me /path/to/file.rs`
2. Browser closes
3. Cursor is at the end of the input
4. Press Enter to send to agent
5. Agent reads and explains the file

## Future Enhancements

Coming soon:
- **/** - Search/filter files by name
- **Ctrl+P** - Quick file picker (fuzzy search)
- **Space** - Toggle file selection (multi-select)
- **d** - Show diff for modified files
- **r** - Refresh file list
- Mouse support for clicking

## Troubleshooting

### Browser doesn't open?
- Make sure you're in Idle state (not during agent response)
- Check you're pressing Ctrl+F (not just F)

### No preview showing?
- Binary files show "[Binary file or read error]"
- Very large files show first 10KB only
- Directories show "[Directory]"

### Git status not showing?
- Make sure you're in a git repository
- Run `git status` in terminal to verify git is working

## Examples

### Example 1: Find and Read a Config File
```
1. Ctrl+F to open browser
2. Navigate to config/
3. Press Enter to expand
4. Find settings.json
5. See preview on right
6. Press Enter to insert path
7. Press Enter to ask agent about it
```

### Example 2: Explore Source Code
```
1. Ctrl+F
2. Navigate to src/
3. Expand with Enter
4. Browse through .rs files
5. Preview shows syntax-highlighted code
6. Find the file you need
7. Press Enter to discuss with agent
```

### Example 3: Check Modified Files
```
1. Ctrl+F
2. Look for yellow M indicators
3. Navigate to modified files
4. Preview shows current content
5. Press Enter to ask agent to review changes
```

---

## 🎉 Enjoy Your New File Browser!

The file browser makes it easy to:
- 📁 Explore your codebase visually
- 👁️ Preview files before opening
- 🎨 See git status at a glance
- ⚡ Quickly access files for discussion

**Press Ctrl+F and start exploring!** 🚀
