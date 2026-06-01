# lau-circuit

> Deadband circuit system — self-contained automations that run within tolerance bands

## What This Does

Deadband circuit system — self-contained automations that run within tolerance bands. Part of the PLATO/LAU ecosystem — a mathematically rigorous framework for building educational agents that learn, teach, and evolve.

## The Key Idea

This crate implements the core abstractions needed for its domain, with a focus on correctness, composability, and conservation guarantees. Every public type is serializable (serde), every algorithm is tested, and every invariant is verified.

## Install

```bash
cargo add lau-circuit
```

## Quick Start

See the API Reference below for complete usage. Key entry points:

```rust
use lau_circuit::*;
// See types and methods below for complete usage
```

## API Reference

```rust
pub enum CircuitState 
pub enum StepAction 
pub struct CircuitStep 
    pub fn new(name: &str, action: StepAction) -> Self 
    pub fn with_on_success(mut self, step_name: &str) -> Self 
    pub fn with_on_failure(mut self, step_name: &str) -> Self 
    pub fn with_max_retries(mut self, n: u32) -> Self 
pub enum DeadbandStatus 
pub enum DeadbandTrend 
pub struct Deadband 
    pub fn new(center: f64, tolerance: f64) -> Self 
    pub fn update(&mut self, tick: u64, value: f64) 
    pub fn status(&self) -> DeadbandStatus 
    pub fn trend(&self) -> DeadbandTrend 
    pub fn ticks_since_breach(&self) -> Option<u64> 
    pub fn distance_to_boundary(&self) -> f64 
    pub fn is_stable(&self, window: usize) -> bool 
pub struct CircuitResult 
pub struct CircuitStatus 
pub struct Circuit 
    pub fn new(name: &str, room_id: &str) -> Self 
    pub fn add_step(&mut self, step: CircuitStep) 
    pub fn set_deadband(&mut self, lower: f64, upper: f64) 
    pub fn tick(&mut self) -> CircuitResult 
    pub fn status(&self) -> CircuitStatus 
    pub fn pause(&mut self) 
    pub fn resume(&mut self) 
    pub fn reset(&mut self) 
    pub fn is_in_deadband(&self) -> bool 
    pub fn error_rate(&self) -> f64 
    pub fn describe(&self) -> String 
pub struct CircuitBuilder 
    pub fn new(name: &str, room: &str) -> Self 
    pub fn step(mut self, s: CircuitStep) -> Self 
    pub fn deadband(mut self, lower: f64, upper: f64) -> Self 
    pub fn tick_interval(mut self, ms: u64) -> Self 
    pub fn automation_level(mut self, level: u32) -> Self 
    pub fn build(self) -> Circuit 
pub fn monitoring_circuit(room: &str, metric: &str, tolerance: f64) -> Circuit 
pub fn course_correction_circuit() -> Circuit 
pub fn motor_calibration_circuit() -> Circuit 
```

## How It Works

Read the source in `src/` for full implementation details. All algorithms are documented with inline comments explaining the mathematical foundations.

## The Math

This crate implements formal mathematical constructs. See the source documentation for theorem statements and proofs of correctness.

## Testing

**66 tests** covering construction, serialization, correctness properties, edge cases, and composability with other lau-* crates.

## License

MIT
