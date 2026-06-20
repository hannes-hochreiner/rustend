use rr.nu

let vscode = (input --numchar 1 "Open VSCode (Y/n)? ")

if ($vscode == "n" or $vscode == "N") {
  print "Not opening VSCode."
} else {
  print "Opening VSCode..."
  ^code .
}