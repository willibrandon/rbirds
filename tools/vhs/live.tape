# One live session in a real terminal (VHS: ttyd and xterm.js, headless),
# run once for cbirds and once for rbirds by tools/vhs/live.sh, which
# replaces PROGRAM and OUT. Each renderer gets the same keys at the same
# times; after each quit the shell checks the terminal settings came back.
Output OUT/PROGRAM.gif
Set Shell "bash"
Set FontSize 14
Set FontFamily "Cascadia Code"
Set Width 1100
Set Height 640
Set TypingSpeed 0
Set Framerate 25

Hide
Type "cd OUT; PS1='$ '; stty -g > PROGRAM.stty; clear" Enter
Show

Type "./PROGRAM --seed 7 --hawks 2 --flocks 2 --depth --trails --panel" Enter
Sleep 3s
Screenshot OUT/PROGRAM-braille-start.png
Sleep 0.5s
Type "hK+e"
Sleep 1.5s
Screenshot OUT/PROGRAM-braille-keys.png
Sleep 0.5s
Type "q"
Sleep 1.5s
Type "echo exit $?; ./settled PROGRAM.stty" Enter
Sleep 0.5s
Screenshot OUT/PROGRAM-braille-quit.png
Sleep 0.5s

Type "clear; ./PROGRAM --seed 7 --render sextants --matrix" Enter
Sleep 3s
Screenshot OUT/PROGRAM-sextants.png
Sleep 0.5s
Ctrl+C
Sleep 1s
Type "echo exit $?; ./settled PROGRAM.stty" Enter
Sleep 0.5s
Screenshot OUT/PROGRAM-sextants-interrupted.png
Sleep 0.5s

Type "clear; ./PROGRAM --seed 7 --render blocks --color acid --shape plane" Enter
Sleep 3s
Screenshot OUT/PROGRAM-blocks.png
Sleep 0.5s
Type "q"
Sleep 1.5s
Type "echo exit $?; ./settled PROGRAM.stty" Enter
Sleep 0.5s
Screenshot OUT/PROGRAM-blocks-quit.png
Sleep 1s
