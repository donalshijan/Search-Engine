#!/bin/bash

# Set the directory containing the files
INPUT_DIR="./documents"  # Change this to your target folder

# Set the output file
OUTPUT_FILE="merged_output.txt"

# Ensure the output file exists (creates it if not)
touch "$OUTPUT_FILE"

# Clear the output file if it already exists
> "$OUTPUT_FILE"

# Loop through all files in the directory
for file in "$INPUT_DIR"/*; do
    if [[ -f "$file" ]]; then
        echo "==== File: $(basename "$file") ====" >> "$OUTPUT_FILE"
        cat "$file" >> "$OUTPUT_FILE"
        echo -e "\n" >> "$OUTPUT_FILE"  # Add a new line for separation
    fi
done

echo "All files have been merged into $OUTPUT_FILE"
