use std::alloc::{alloc, Layout};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Instant};
use rand::prelude::*;
use clap::Parser;
use rayon::prelude::*;
use libc;
use num_cpus;

#[cfg(target_os = "linux")]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(target_os = "macos")]
use std::os::unix::io::AsRawFd;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long)]
    dir: PathBuf,

    #[arg(short, long)]
    threads: Option<usize>,
}

const CPU_FIBONACCI_IT : u32 = 42;

const SHARED_MEMORY_SIZE : usize = 4 * 1024 * 1024 * 1024;

const SHARED_MEMORY_ITERATIONS_MUTEX : u32 = 2_000_000;
const SHARED_MEMORY_ITERATIONS_ATOMIC : u32 = 20_000_000;

const IO_FILE_SIZE : usize = 4 * 1024 * 1024 * 1024;
const IO_READ_ITERATIONS : u32 = 20_000;

const FS_ITERATIONS: u32 = 50_000;


fn main() {
    let args = Args::parse();
    let num_threads = args.threads.unwrap_or_else(|| num_cpus::get() / 2);
    
    // Configure global thread pool
    rayon::ThreadPoolBuilder::new().num_threads(num_threads).build_global().unwrap();

    println!("--- Starting Benchmark (Threads: {}) ---", num_threads);

    // 1. CPU Bound: Fibonacci / Heavy Calculation
    bench_section("CPU Bound (Fibonacci)", || cpu_work(CPU_FIBONACCI_IT), num_threads);

    // 2. Memory Bandwidth: Large Array Operations
    bench_section("Memory Bandwidth (non-shared)", || memory_test_unshared(100_000_000), num_threads);

    memory_test_mutex(num_threads);
    memory_test_atomic(num_threads);

    // 3. IO: Sequential and Random
    io_benchmarks(&args.dir, num_threads);

    // 4. Filesystem: Create/Delete Metadata
    fs_benchmarks(&args.dir, num_threads);
}

// --- BENCHMARK LOGIC ---

fn cpu_work(n: u32) {
    fn fib(n: u32) -> u32 {
        if n <= 1 { n } else { fib(n - 1) + fib(n - 2) }
    }
    let mut _ret = fib(n);
}

fn memory_test_unshared(size: usize) {
    let mut data = vec![1.0f64; size];
    for i in 0..size {
        data[i] = data[i] * 2.5 + 1.2;
    }
}

fn memory_test_mutex(num_threads: usize) {
    println!("\n[Memory shared access (mutex)]");
    println!("  Allocating");
    let data = Arc::new(Mutex::new(vec![0u8; SHARED_MEMORY_SIZE]));

    let mut handles = vec![];

    let start = Instant::now();

    println!("  Starting");

    for _t in 0..num_threads {
        let shared_data = Arc::clone(&data);
        
        let handle = thread::spawn(move || {
            let mut rng = rand::rng();
            //println!("Thread {} started.", t);

            for _ in 0..SHARED_MEMORY_ITERATIONS_MUTEX {
                // Randomly pick a block index
                let idx = rng.random_range(0..SHARED_MEMORY_SIZE - 64);
                
                // Lock the mutex to get access
                let mut mem = shared_data.lock().unwrap();
                
                // Randomly Write or Read
                if rng.random_bool(0.5) {
                    let random_number: u8 = rng.random();
                    mem[idx] = random_number;
                } else {
                    let _val = mem[idx]; // Read
                }
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }
    println!("  Memory shared access (mutex): {:?}", start.elapsed());
}

fn memory_test_atomic(num_threads: usize) {
     println!("\n[Memory shared access (atomic)]");
    let num_elements = SHARED_MEMORY_SIZE / 8; // Since AtomicU64 is 8 bytes
    
    println!("  Allocating");
    let data = Arc::new(unsafe {
        let layout = std::alloc::Layout::from_size_align(SHARED_MEMORY_SIZE, 4096).unwrap();
        let ptr = std::alloc::alloc_zeroed(layout) as *mut AtomicU64;
        Vec::from_raw_parts(ptr, num_elements, num_elements)
    });

    println!("  Starting");

    let start = Instant::now();

    let mut handles = vec![];
    for _t in 0..num_threads {
        let shared_data = Arc::clone(&data);
        handles.push(thread::spawn(move || {
            let mut rng = rand::rng();
            for _ in 0..SHARED_MEMORY_ITERATIONS_ATOMIC {
                let idx = rng.random_range(0..num_elements);
                if rng.random_bool(0.5) {
                    // Relaxed ordering is fastest; doesn't enforce cross-CPU synchronization
                    shared_data[idx].store(rng.random(), Ordering::Relaxed);
                } else {
                    let _val = shared_data[idx].load(Ordering::Relaxed);
                }
            }
        }));
    }

    for h in handles { h.join().unwrap(); }

    println!("  Memory shared access (atomic): {:?}", start.elapsed());
}

fn io_benchmarks(path: &Path, threads: usize) {
    println!("\n[IO Performance]");
    let file_path = path.join("bench_large.bin");
    let size = IO_FILE_SIZE;

    // Mono-thread Sequential Write
    let start = Instant::now();
    {
        let mut f = File::create(&file_path).unwrap();
        let buf = vec![0u8; 1024 * 64];
        for _ in 0..(size / buf.len()) {
            f.write_all(&buf).unwrap();
        }
    }
    println!("  Sequential Write (Mono): {:?}", start.elapsed());

    // Multi-threaded Random Read
    let start = Instant::now();
    (0..threads).into_par_iter().for_each(|_| {
        const DIRECT_BLOCK_SIZE : usize = 4096;

        let mut f = open_with_direct_io(&file_path).unwrap();
        let mut rng = rand::rng();

        let valid_positions : usize = size / DIRECT_BLOCK_SIZE;

        unsafe {
            let layout = Layout::from_size_align(DIRECT_BLOCK_SIZE, DIRECT_BLOCK_SIZE).unwrap();
            let ptr = alloc(layout);
            let mut buf = std::slice::from_raw_parts_mut(ptr, DIRECT_BLOCK_SIZE);

            for _it in 0..IO_READ_ITERATIONS {
                let pos = rng.random_range(0..valid_positions as usize);
                f.seek(SeekFrom::Start((pos * DIRECT_BLOCK_SIZE) as u64)).unwrap();
                f.read_exact(&mut buf).unwrap();
            }
        }
    });
    println!("  Random Read Direct (Multi): {:?}", start.elapsed());
    
    let _ = fs::remove_file(file_path);
}

fn fs_benchmarks(path: &Path, threads: usize) {
    println!("\n[Filesystem Metadata]");
    let base = path.join("fs_test");
    fs::create_dir_all(&base).unwrap();

    let start = Instant::now();
    (0..threads).into_par_iter().for_each(|t| {
        let thread_dir = base.join(format!("t_{}", t));
        fs::create_dir(&thread_dir).unwrap();
        for i in 0..FS_ITERATIONS {
            let f_path = thread_dir.join(format!("{}.txt", i));
            File::create(&f_path).unwrap();
            fs::remove_file(&f_path).unwrap();
        }
    });
    println!("  Create/Delete files/thread: {:?}", start.elapsed());
    let _ = fs::remove_dir_all(base);
}

// --- UTILS ---

fn bench_section<F>(name: &str, f: F, threads: usize) 
where F: Fn() + Sync + Send + Copy {
    println!("\n[{}]", name);
    
    let start = Instant::now();
    f();
    println!("  Mono-thread:  {:?}", start.elapsed());

    let start = Instant::now();
    (0..threads).into_par_iter().for_each(|_| f());
    println!("  Multi-thread: {:?}", start.elapsed());
}


fn open_with_direct_io(path: &PathBuf) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);

    // --- Linux Logic ---
    #[cfg(target_os = "linux")]
    {
        options.custom_flags(libc::O_DIRECT);
        options.open(path)
    }

    // --- macOS Logic ---
    #[cfg(target_os = "macos")]
    {
        let file = options.open(path).unwrap();
        let fd = file.as_raw_fd();
        
        unsafe {
            // F_NOCACHE turns off the page cache for this file descriptor
            if libc::fcntl(fd, libc::F_NOCACHE, 1) == -1 {
                println!("fcntl failed");
                return Err(std::io::Error::last_os_error());
            } else {
            }
        }
        Ok(file)
    }

    // --- Fallback for other OSs ---
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        options.open(path)
    }
}
