# Benchy micro benchmark

To build

```
cargo build --release
```


To run

```
mkdir test
target/release/benchy --dir test --threads 4
```

If not set, threads defaults to num_cpus / 2


Sample output (AWS m6i, 4 threads, GP2 EBS):

```
--- Starting Benchmark (Threads: 4) ---

[CPU Bound (Fibonacci)]
  Mono-thread:  725.633779ms
  Multi-thread: 722.857073ms

[Memory Bandwidth (non-shared)]
  Mono-thread:  403.040447ms
  Multi-thread: 595.586279ms

[Memory shared access (mutex)]
  Allocating
  Starting
  Memory shared access (mutex): 3.068460613s

[Memory shared access (atomic)]
  Allocating
  Starting
  Memory shared access (atomic): 1.005242381s

[IO Performance]
  Sequential Write (Mono): 1.267140099s
  Random Read Direct (Multi): 34.918842088s

[Filesystem Metadata]
  Create/Delete files/thread: 644.421337ms
```
