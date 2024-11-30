#!/bin/bash

# Compile both executables
echo "Compiling the project..."
cargo build
cargo build --release

# Log file setup
LOG_FILE="search_engine.logs"
if [ -f "$LOG_FILE" ]; then
    echo "Log file exists. Deleting and recreating: $LOG_FILE"
    > "$LOG_FILE"
else
    echo "Log file does not exist. Creating: $LOG_FILE"
    touch "$LOG_FILE"
fi

# File to store results
RESULT_FILE="results.txt"
echo "Creating/clearing result file: $RESULT_FILE"
> "$RESULT_FILE"

# Engine modes
ENGINE_MODES=("single_core_single_thread" "multi_core_single_thread" "multi_core_multiple_threads_each_thread_searching_against_whole_index" "multi_core_multiple_threads_each_thread_in_each_core_searching_against_single_sharded_subset_of_index")

# Run tests for each engine mode
for MODE in "${ENGINE_MODES[@]}"; do
    echo "Running tests for mode: $MODE"

    # Write engine mode to result file
    echo "$MODE" >> "$RESULT_FILE"

    # Run search engine
    echo "Starting search engine..."
    ./target/debug/search_engine ./documents "$MODE" > "$LOG_FILE" 2>&1 &
    SEARCH_ENGINE_PID=$!

    # Wait for search engine to start up (adjust sleep time if needed)
    sleep 5

    # Run the test executable
    echo "Running test executable..."
    ./target/debug/test
    if [ $? -eq 0 ]; then
        echo "Test finished successfully."
    else
        echo "Test failed."
    fi

    # Shut down the search engine
    echo "Shutting down search engine..."
    kill -s SIGINT "$SEARCH_ENGINE_PID"
    wait "$SEARCH_ENGINE_PID"

    # Add a delay to ensure clean shutdown (adjust if needed)
    sleep 2
done

echo "All tests completed."
