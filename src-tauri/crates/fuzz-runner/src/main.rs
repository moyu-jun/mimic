use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::time::Instant;

const DEFAULT_ITERATIONS: u64 = 100_000;
const DEFAULT_MAX_LEN: usize = 256 * 1024;
const DEFAULT_SEED: u64 = 0x6d69_6d69_6320_6675;

const CONFIG_SEEDS: &[&[u8]] = &[
    include_bytes!("../../../fuzz/corpus/config/default.ini"),
    b"",
    b"[hotkeys]
start_label=F12
start_scan_code=88
",
];
const WAV_SEEDS: &[&[u8]] = &[
    include_bytes!("../../../../extra/audio/按键开启.wav"),
    include_bytes!("../../../../extra/audio/按键关闭.wav"),
    b"",
    b"RIFF\x00\x00\x00\x00WAVE",
];

#[derive(Clone, Copy)]
struct Options {
    iterations: u64,
    max_len: usize,
    seed: u64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("fuzz runner failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let options = parse_options()?;
    let started = Instant::now();
    let mut random = options.seed;

    for iteration in 0..options.iterations {
        let config_target = next_random(&mut random) & 1 == 0;
        let seeds = if config_target {
            CONFIG_SEEDS
        } else {
            WAV_SEEDS
        };
        let seed_index = (next_random(&mut random) as usize) % seeds.len();
        let mut input = seeds[seed_index].to_vec();
        let mutation_count = 1 + (next_random(&mut random) % 8);
        for _ in 0..mutation_count {
            mutate(&mut input, options.max_len, &mut random);
        }

        let outcome = catch_unwind(AssertUnwindSafe(|| {
            if config_target {
                mimic_lib::fuzzing::config_bytes(&input);
            } else {
                mimic_lib::fuzzing::wav_bytes(&input);
            }
        }));
        if outcome.is_err() {
            let target = if config_target { "config" } else { "wav" };
            let artifact = save_artifact(target, options.seed, iteration, &input)?;
            return Err(format!(
                "{target} parser panicked at iteration {iteration}; artifact: {}",
                artifact.display()
            ));
        }
    }

    let elapsed = started.elapsed();
    let rate = options.iterations as f64 / elapsed.as_secs_f64().max(0.001);
    println!(
        "fuzz completed: {} iterations, seed {}, {:.0} cases/s, {:.2}s",
        options.iterations,
        options.seed,
        rate,
        elapsed.as_secs_f64()
    );
    Ok(())
}

fn parse_options() -> Result<Options, String> {
    let mut options = Options {
        iterations: DEFAULT_ITERATIONS,
        max_len: DEFAULT_MAX_LEN,
        seed: DEFAULT_SEED,
    };
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| format!("missing value for {argument}"))?;
        match argument.as_str() {
            "--iterations" => {
                options.iterations = value
                    .parse()
                    .map_err(|_| "iterations must be a positive integer".to_string())?;
                if options.iterations == 0 {
                    return Err("iterations must be greater than zero".to_string());
                }
            }
            "--max-len" => {
                options.max_len = value
                    .parse()
                    .map_err(|_| "max-len must be a positive integer".to_string())?;
                if options.max_len == 0 || options.max_len > 5 * 1024 * 1024 {
                    return Err("max-len must be between 1 and 5242880".to_string());
                }
            }
            "--seed" => {
                options.seed = value
                    .parse()
                    .map_err(|_| "seed must be an unsigned integer".to_string())?;
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    Ok(options)
}

fn mutate(input: &mut Vec<u8>, max_len: usize, random: &mut u64) {
    match next_random(random) % 7 {
        0 if !input.is_empty() => {
            let index = (next_random(random) as usize) % input.len();
            input[index] ^= 1 << (next_random(random) % 8);
        }
        1 if input.len() < max_len => {
            let index = (next_random(random) as usize) % (input.len() + 1);
            input.insert(index, next_random(random) as u8);
        }
        2 if !input.is_empty() => {
            let index = (next_random(random) as usize) % input.len();
            input.remove(index);
        }
        3 if !input.is_empty() => {
            let new_len = (next_random(random) as usize) % input.len();
            input.truncate(new_len);
        }
        4 if input.len() < max_len => {
            let count = (1 + next_random(random) % 32) as usize;
            let available = max_len - input.len();
            input.extend((0..count.min(available)).map(|_| next_random(random) as u8));
        }
        5 if input.len() >= 4 => {
            let index = (next_random(random) as usize) % (input.len() - 3);
            let value = (next_random(random) as u32).to_le_bytes();
            input[index..index + 4].copy_from_slice(&value);
        }
        _ if !input.is_empty() && input.len() < max_len => {
            let start = (next_random(random) as usize) % input.len();
            let count = (1 + next_random(random) as usize % 32)
                .min(input.len() - start)
                .min(max_len - input.len());
            input.extend_from_within(start..start + count);
        }
        _ if input.len() < max_len => input.push(next_random(random) as u8),
        _ if !input.is_empty() => {
            let index = (next_random(random) as usize) % input.len();
            input[index] = next_random(random) as u8;
        }
        _ => {}
    }
}

fn next_random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn save_artifact(target: &str, seed: u64, iteration: u64, input: &[u8]) -> Result<PathBuf, String> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fuzz")
        .join("artifacts");
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("failed to create artifact directory: {error}"))?;
    let path = directory.join(format!("{target}-{seed}-{iteration}.bin"));
    std::fs::write(&path, input)
        .map_err(|error| format!("failed to write crash artifact: {error}"))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutator_respects_length_limit() {
        let mut input = vec![0; 8];
        let mut random = DEFAULT_SEED;
        for _ in 0..10_000 {
            mutate(&mut input, 64, &mut random);
            assert!(input.len() <= 64);
        }
    }

    #[test]
    fn parser_corpus_is_panic_free() {
        for input in CONFIG_SEEDS {
            mimic_lib::fuzzing::config_bytes(input);
        }
        for input in WAV_SEEDS {
            mimic_lib::fuzzing::wav_bytes(input);
        }
    }
}
