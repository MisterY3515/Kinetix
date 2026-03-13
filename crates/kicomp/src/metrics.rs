/// Kinetix Compiler Metrics — Build 35
/// Collects and reports optimization statistics.

use std::time::Instant;

/// Compiler metrics collector.
#[derive(Debug, Clone)]
pub struct CompilerMetrics {
    pub phases: Vec<PhaseMetric>,
    pub total_instructions_before: usize,
    pub total_instructions_after: usize,
    pub monomorphization_count: usize,
    pub trait_cache_hits: usize,
    pub trait_cache_misses: usize,
}

#[derive(Debug, Clone)]
pub struct PhaseMetric {
    pub name: String,
    pub duration_ms: f64,
    pub instructions_before: usize,
    pub instructions_after: usize,
}

impl CompilerMetrics {
    pub fn new() -> Self {
        Self {
            phases: Vec::new(),
            total_instructions_before: 0,
            total_instructions_after: 0,
            monomorphization_count: 0,
            trait_cache_hits: 0,
            trait_cache_misses: 0,
        }
    }

    pub fn record_phase(&mut self, name: &str, before: usize, after: usize, duration_ms: f64) {
        self.phases.push(PhaseMetric {
            name: name.to_string(),
            duration_ms,
            instructions_before: before,
            instructions_after: after,
        });
    }

    /// Count total instructions in a compiled program.
    pub fn count_instructions(program: &crate::ir::CompiledProgram) -> usize {
        let mut total = program.main.instructions.len();
        for func in &program.functions {
            total += func.instructions.len();
        }
        total
    }

    pub fn print_report(&self) {
        eprintln!();
        eprintln!("\x1b[1;36m╔══════════════════════════════════════════════════╗\x1b[0m");
        eprintln!("\x1b[1;36m║         Kinetix Compiler Metrics (Build 35)      ║\x1b[0m");
        eprintln!("\x1b[1;36m╚══════════════════════════════════════════════════╝\x1b[0m");
        eprintln!();

        if !self.phases.is_empty() {
            eprintln!("\x1b[1;33m  Optimization Passes:\x1b[0m");
            for phase in &self.phases {
                let delta = phase.instructions_before as i64 - phase.instructions_after as i64;
                let pct = if phase.instructions_before > 0 {
                    (delta as f64 / phase.instructions_before as f64) * 100.0
                } else {
                    0.0
                };
                eprintln!(
                    "    \x1b[37m{:<30}\x1b[0m {:>5} → {:>5} instrs  \x1b[32m(-{:.1}%)\x1b[0m  {:.2}ms",
                    phase.name,
                    phase.instructions_before,
                    phase.instructions_after,
                    pct,
                    phase.duration_ms
                );
            }
            eprintln!();
        }

        eprintln!("\x1b[1;33m  Summary:\x1b[0m");
        eprintln!("    Instructions before opt: {}", self.total_instructions_before);
        eprintln!("    Instructions after opt:  {}", self.total_instructions_after);
        let total_delta = self.total_instructions_before as i64 - self.total_instructions_after as i64;
        let total_pct = if self.total_instructions_before > 0 {
            (total_delta as f64 / self.total_instructions_before as f64) * 100.0
        } else {
            0.0
        };
        eprintln!("    Total reduction:         \x1b[1;32m{} instructions ({:.1}%)\x1b[0m", total_delta, total_pct);

        if self.monomorphization_count > 0 {
            eprintln!("    Monomorphizations:       {}", self.monomorphization_count);
        }
        if self.trait_cache_hits + self.trait_cache_misses > 0 {
            let hit_rate = (self.trait_cache_hits as f64 / (self.trait_cache_hits + self.trait_cache_misses) as f64) * 100.0;
            eprintln!("    Trait cache hit rate:     {:.1}% ({}/{})", hit_rate, self.trait_cache_hits, self.trait_cache_hits + self.trait_cache_misses);
        }
        eprintln!();
    }
}

/// Convenience: time a closure and return (result, duration_ms).
pub fn timed<F, R>(f: F) -> (R, f64)
where
    F: FnOnce() -> R,
{
    let start = Instant::now();
    let result = f();
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
    (result, elapsed)
}
