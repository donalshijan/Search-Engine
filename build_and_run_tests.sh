#!/bin/bash


# Array to store PIDs of background processes
BACKGROUND_PIDS=()

# Function to track a background process
track_background_process() {
    BACKGROUND_PIDS+=("$1")
}

# Cleanup function
cleanup() {
    echo "Cleaning up..."
    for PID in "${BACKGROUND_PIDS[@]}"; do
        if kill -0 "$PID" 2>/dev/null; then
            echo "Killing process with PID: $PID"
            kill -s SIGKILL "$PID" 2>/dev/null
        fi
    done
    echo "Cleanup complete."
}


# Trap signals to call the cleanup function
trap cleanup EXIT SIGINT SIGTERM

# Compile both executables
echo "Compiling the project..."
cargo build
if [ $? -ne 0 ]; then
    echo "Cargo build (debug) failed. Exiting."
    cleanup
    exit 1
fi

cargo build --release
if [ $? -ne 0 ]; then
    echo "Cargo build (release) failed. Exiting."
    cleanup
    exit 1
fi

TEST_QUERY_PROCESSING_TIME_LOG_FILE="test.log"
if [ -f "$TEST_QUERY_PROCESSING_TIME_LOG_FILE" ]; then
    echo "Log file exists. Deleting: $TEST_QUERY_PROCESSING_TIME_LOG_FILE"
    rm "$TEST_QUERY_PROCESSING_TIME_LOG_FILE"
fi

# Create a new log file
echo "Creating new log file: $TEST_QUERY_PROCESSING_TIME_LOG_FILE"
touch "$TEST_QUERY_PROCESSING_TIME_LOG_FILE"

LOG_FILE="search_engine.log"
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
    echo "$MODE" >> "$TEST_QUERY_PROCESSING_TIME_LOG_FILE"

    # Run search engine
    echo "Starting search engine..."
    ./target/debug/search_engine ./documents "$MODE" testing > "$LOG_FILE" 2>&1 &
    SEARCH_ENGINE_PID=$!
    track_background_process "$SEARCH_ENGINE_PID"
    # Wait for search engine to start up (adjust sleep time if needed)
    sleep 2

    # Run the test executable
    echo "Running test executable..."
    ./target/debug/test
    if [ $? -eq 0 ]; then
        echo "Test finished for $MODE mode."
    else
        echo "Test failed for $MODE mode."
    fi

    # Shut down the search engine
    echo "Shutting down search engine..."
    kill -s SIGINT "$SEARCH_ENGINE_PID"
    wait "$SEARCH_ENGINE_PID" 2>/dev/null
done

echo "All tests completed."
# pkill -9 search_engine 