# prompt

Reads files from the current directory into a prompt, respecting .gitignore and .promptignore files.

## Basic usage

```
prompt  # print to stdout
prompt --copy  # copies straight to clipboard
```

## Suggested .promptignore in home directory

```
# Images
*.png
*.jpg
*.jpeg
*.gif
*.bmp
*.tiff
*.ico
*.webp
*.svgz

# Audio
*.mp3
*.wav
*.ogg
*.flac

# Video
*.mp4
*.mkv
*.mov
*.avi
*.wmv

# Archives / Compressed files
*.zip
*.tar
*.gz
*.bz2
*.7z
*.rar

# Documents (binary/non-text)
*.pdf
*.doc
*.docx
*.xls
*.xlsx
*.ppt
*.pptx

# Executables
*.exe
*.dll
*.so
*.dylib
*.bin
*.dat

# Fonts
*.ttf
*.otf
*.woff
*.woff2
```
