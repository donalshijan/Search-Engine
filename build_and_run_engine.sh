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


    ./target/debug/search_engine ./documents multi_core_multiple_threads_each_thread_in_each_core_searching_against_single_sharded_subset_of_index production 
# pkill -9 search_engine 