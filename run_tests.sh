#!/bin/bash

# Compile both executables
echo "Compiling the project..."
cargo build
cargo build --release

# File to store results
RESULT_FILE="result.txt"
echo "Creating/clearing result file: $RESULT_FILE"
> "$RESULT_FILE"

# Engine modes
ENGINE_MODES=("single_core_single_thread" "multi_core_multiple_threads_each_thread_searching_against_whole_index" "multi_core_multiple_threads_each_thread_in_same_core_searching_against_single_sharded_subset_of_index")

# Run tests for each engine mode
for MODE in "${ENGINE_MODES[@]}"; do
    echo "Running tests for mode: $MODE" | tee -a "$RESULT_FILE"

    # Write engine mode to result file
    echo "$MODE" >> "$RESULT_FILE"

    # Run search engine
    echo "Starting search engine..."
    ./target/debug/search_engine ./documents "$MODE" &
    SEARCH_ENGINE_PID=$!

    # Wait for search engine to start up (adjust sleep time if needed)
    sleep 5

    # Run the test executable
    echo "Running test executable..."
    ./target/debug/main
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
