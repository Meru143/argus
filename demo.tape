# Argus Demo Tape
# Run with: vhs demo.tape

Output demo.gif

Set FontSize 18
Set Width 1200
Set Height 800
Set Padding 20
Set FontFamily "JetBrains Mono"
Set Theme "Catppuccin Mocha"

# Setup
Hide
Type "export PS1='➜  argus git:(main) ✗ '"
Enter
Type "clear"
Enter
Show

# 1. Intro
Type "# Argus: Your AI Code Reviewer"
Sleep 1s
Enter
Sleep 500ms
Type "# 1. Understand your codebase structure"
Sleep 500ms
Enter

# 2. Map
Type "argus map --path . --max-tokens 500"
Sleep 500ms
Enter
Sleep 3s

# 3. Review
Type "# 2. Review your changes with context"
Sleep 500ms
Enter
Type "git diff HEAD~1 | argus review --repo ."
Sleep 500ms
Enter

# Wait for "AI output" simulation (since we can't really call LLM in VHS easily without keys)
# For the real recording, you'll need the API key set.
# If you don't want to burn tokens, we can mock the output or just cut the video.
Sleep 10s

# End
Type "# Done! Local, fast, and context-aware."
Sleep 1s
Enter
Sleep 3s
