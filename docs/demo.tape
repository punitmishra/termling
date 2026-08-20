# docs/demo.tape — regenerate the README demo GIF with VHS:
#
#     cargo build --release      # the tape runs the release binary
#     vhs docs/demo.tape         # writes docs/demo.gif
#
# `termling demo` feeds a scripted timeline of synthetic shell events into a
# fresh companion (no real shell, no socket) so the creature parades through
# every mood and flair, then exits on its own after ~20s.

Output docs/demo.gif

Set Shell "zsh"
Set FontSize 16
Set Width 780
Set Height 540
Set Padding 16
Set Framerate 20

Hide
Type "export PATH=$PWD/target/release:$PATH" Enter
Type "clear" Enter
Show
Sleep 500ms
Type "termling demo"
Sleep 400ms
Enter
Sleep 22s
