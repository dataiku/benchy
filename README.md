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