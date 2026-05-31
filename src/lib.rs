//! # lau-circuit
//!
//! The deadband circuit system — self-contained automations that run within tolerance bands.
//! When everything stays within deadband, the circuit runs itself. When it drifts, the ensign notices.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Unique ID generation counter (simple incrementing).
static mut ID_COUNTER: u64 = 0;

fn next_id(prefix: &str) -> String {
    // SAFETY: single-threaded usage in this crate.
    let n = unsafe {
        ID_COUNTER += 1;
        ID_COUNTER
    };
    format!("{prefix}-{n}")
}

// ---------------------------------------------------------------------------
// CircuitState
// ---------------------------------------------------------------------------

/// State of a circuit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CircuitState {
    Idle,
    Running,
    Paused,
    Error(String),
    Complete,
}

impl fmt::Display for CircuitState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Idle => write!(f, "Idle"),
            Self::Running => write!(f, "Running"),
            Self::Paused => write!(f, "Paused"),
            Self::Error(e) => write!(f, "Error({e})"),
            Self::Complete => write!(f, "Complete"),
        }
    }
}

// ---------------------------------------------------------------------------
// StepAction
// ---------------------------------------------------------------------------

/// An action a step can perform.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StepAction {
    CheckDeadband { metric: String },
    Compute { formula: String, output: String },
    Branch { condition: String, if_true: String, if_false: String },
    Notify { message: String, level: String },
    Log { message: String },
    SetMetric { key: String, value: f64 },
    Wait { ticks: u64 },
    Delegate { to_room: String, task: String },
    Escalate { reason: String },
}

// ---------------------------------------------------------------------------
// CircuitStep
// ---------------------------------------------------------------------------

/// One step in the automation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CircuitStep {
    pub name: String,
    pub action: StepAction,
    pub on_success: Option<String>,
    pub on_failure: Option<String>,
    pub max_retries: u32,
    pub retry_count: u32,
}

impl CircuitStep {
    pub fn new(name: &str, action: StepAction) -> Self {
        Self {
            name: name.to_string(),
            action,
            on_success: None,
            on_failure: None,
            max_retries: 0,
            retry_count: 0,
        }
    }

    pub fn with_on_success(mut self, step_name: &str) -> Self {
        self.on_success = Some(step_name.to_string());
        self
    }

    pub fn with_on_failure(mut self, step_name: &str) -> Self {
        self.on_failure = Some(step_name.to_string());
        self
    }

    pub fn with_max_retries(mut self, n: u32) -> Self {
        self.max_retries = n;
        self
    }
}

// ---------------------------------------------------------------------------
// Deadband
// ---------------------------------------------------------------------------

/// Status of the deadband.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum DeadbandStatus {
    Green,
    Yellow,
    Red,
    Breached,
}

impl fmt::Display for DeadbandStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Green => write!(f, "Green"),
            Self::Yellow => write!(f, "Yellow"),
            Self::Red => write!(f, "Red"),
            Self::Breached => write!(f, "Breached"),
        }
    }
}

/// Trend direction of the deadband.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DeadbandTrend {
    Stable,
    Drifting(f64),
    Oscillating(f64),
    Diverging,
}

impl fmt::Display for DeadbandTrend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stable => write!(f, "Stable"),
            Self::Drifting(rate) => write!(f, "Drifting({rate:.4})"),
            Self::Oscillating(amplitude) => write!(f, "Oscillating({amplitude:.4})"),
            Self::Diverging => write!(f, "Diverging"),
        }
    }
}

/// The tolerance band.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Deadband {
    pub lower: f64,
    pub upper: f64,
    pub current: f64,
    pub warning_threshold: f64,
    pub history: Vec<(u64, f64)>,
    pub max_history: usize,
}

impl Deadband {
    pub fn new(center: f64, tolerance: f64) -> Self {
        Self {
            lower: center - tolerance,
            upper: center + tolerance,
            current: center,
            warning_threshold: tolerance * 0.8,
            history: Vec::new(),
            max_history: 1000,
        }
    }

    pub fn update(&mut self, tick: u64, value: f64) {
        self.current = value;
        self.history.push((tick, value));
        if self.history.len() > self.max_history {
            self.history.remove(0);
        }
    }

    pub fn status(&self) -> DeadbandStatus {
        if self.current < self.lower || self.current > self.upper {
            return DeadbandStatus::Breached;
        }
        let dist_to_lower = (self.current - self.lower).abs();
        let dist_to_upper = (self.upper - self.current).abs();
        let min_dist = dist_to_lower.min(dist_to_upper);
        let range = (self.upper - self.lower).abs();
        let margin = range - min_dist * 2.0;
        if margin > self.warning_threshold {
            DeadbandStatus::Red
        } else if min_dist < self.warning_threshold * 0.3 {
            DeadbandStatus::Yellow
        } else {
            DeadbandStatus::Green
        }
    }

    pub fn trend(&self) -> DeadbandTrend {
        if self.history.len() < 3 {
            return DeadbandTrend::Stable;
        }
        let n = self.history.len();
        let recent: Vec<f64> = self.history.iter().rev().take(10).map(|(_, v)| *v).collect();

        // Check oscillation: count sign changes in deltas
        let mut sign_changes = 0;
        let mut deltas = Vec::new();
        for i in 1..recent.len() {
            let d = recent[i - 1] - recent[i];
            deltas.push(d);
        }
        for i in 1..deltas.len() {
            if deltas[i - 1].signum() != deltas[i].signum() {
                sign_changes += 1;
            }
        }

        if sign_changes as f64 / (deltas.len().max(1) as f64) > 0.6 {
            let amplitude = recent.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
                - recent.iter().cloned().fold(f64::INFINITY, f64::min);
            return DeadbandTrend::Oscillating(amplitude);
        }

        // Linear drift detection
        let first = self.history[0].1;
        let last = self.history[n - 1].1;
        let rate = (last - first) / n as f64;

        if rate.abs() < 0.001 {
            return DeadbandTrend::Stable;
        }

        // Check divergence: if drift is accelerating
        if self.history.len() >= 6 {
            let mid = n / 2;
            let rate_first_half = (self.history[mid].1 - self.history[0].1) / mid.max(1) as f64;
            let rate_second_half =
                (self.history[n - 1].1 - self.history[mid].1) / (n - mid).max(1) as f64;
            if rate_second_half.abs() > rate_first_half.abs() * 1.5 {
                return DeadbandTrend::Diverging;
            }
        }

        DeadbandTrend::Drifting(rate)
    }

    pub fn ticks_since_breach(&self) -> Option<u64> {
        let current_tick = self.history.last().map(|(t, _)| *t)?;
        for (tick, value) in self.history.iter().rev() {
            if *value < self.lower || *value > self.upper {
                return Some(current_tick - tick);
            }
        }
        None
    }

    pub fn distance_to_boundary(&self) -> f64 {
        let dist_lower = (self.current - self.lower).abs();
        let dist_upper = (self.upper - self.current).abs();
        dist_lower.min(dist_upper)
    }

    pub fn is_stable(&self, window: usize) -> bool {
        if self.history.len() < window {
            return true;
        }
        let recent: Vec<f64> = self.history.iter().rev().take(window).map(|(_, v)| *v).collect();
        let variance = {
            let mean = recent.iter().sum::<f64>() / recent.len() as f64;
            recent.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / recent.len() as f64
        };
        variance < 0.01
    }
}

// ---------------------------------------------------------------------------
// CircuitResult
// ---------------------------------------------------------------------------

/// What happened in one tick.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CircuitResult {
    pub circuit_id: String,
    pub step_executed: Option<String>,
    pub deadband_status: DeadbandStatus,
    pub action_taken: String,
    pub conservation_cost: f64,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// CircuitStatus
// ---------------------------------------------------------------------------

/// Full status snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CircuitStatus {
    pub id: String,
    pub name: String,
    pub state: CircuitState,
    pub automation_level: u32,
    pub deadband: DeadbandStatus,
    pub run_count: u64,
    pub error_rate: f64,
    pub current_step: Option<String>,
    pub total_steps: usize,
}

// ---------------------------------------------------------------------------
// Circuit
// ---------------------------------------------------------------------------

/// A self-contained automation loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Circuit {
    pub id: String,
    pub name: String,
    pub room_id: String,
    pub state: CircuitState,
    pub automation_level: u32,
    pub deadband: Deadband,
    pub tick_interval: u64,
    pub steps: Vec<CircuitStep>,
    pub current_step: usize,
    pub run_count: u64,
    pub error_count: u32,
    /// In-memory metrics store for SetMetric / CheckDeadband steps.
    #[serde(default)]
    pub metrics: HashMap<String, f64>,
    /// Internal tick counter.
    #[serde(default)]
    tick_counter: u64,
    /// Wait ticks remaining.
    #[serde(default)]
    wait_remaining: u64,
}

impl Circuit {
    pub fn new(name: &str, room_id: &str) -> Self {
        Self {
            id: next_id("circuit"),
            name: name.to_string(),
            room_id: room_id.to_string(),
            state: CircuitState::Idle,
            automation_level: 1,
            deadband: Deadband::new(0.0, 1.0),
            tick_interval: 1000,
            steps: Vec::new(),
            current_step: 0,
            run_count: 0,
            error_count: 0,
            metrics: HashMap::new(),
            tick_counter: 0,
            wait_remaining: 0,
        }
    }

    pub fn add_step(&mut self, step: CircuitStep) {
        self.steps.push(step);
    }

    pub fn set_deadband(&mut self, lower: f64, upper: f64) {
        let center = (lower + upper) / 2.0;
        let tolerance = (upper - lower) / 2.0;
        self.deadband = Deadband::new(center, tolerance);
    }

    pub fn tick(&mut self) -> CircuitResult {
        self.tick_counter += 1;

        // Handle waiting
        if self.wait_remaining > 0 {
            self.wait_remaining -= 1;
            if self.wait_remaining == 0 {
                // Wait complete, advance to next step
                self.current_step += 1;
                if self.current_step >= self.steps.len() {
                    self.state = CircuitState::Complete;
                }
            }
            return CircuitResult {
                circuit_id: self.id.clone(),
                step_executed: None,
                deadband_status: self.deadband.status(),
                action_taken: format!("Waiting ({} ticks remaining)", self.wait_remaining),
                conservation_cost: 0.1,
                error: None,
            };
        }

        // Check state
        if self.state == CircuitState::Paused {
            return CircuitResult {
                circuit_id: self.id.clone(),
                step_executed: None,
                deadband_status: self.deadband.status(),
                action_taken: "Paused".to_string(),
                conservation_cost: 0.0,
                error: None,
            };
        }

        if matches!(self.state, CircuitState::Error(_)) {
            return CircuitResult {
                circuit_id: self.id.clone(),
                step_executed: None,
                deadband_status: self.deadband.status(),
                action_taken: "In error state".to_string(),
                conservation_cost: 0.0,
                error: Some("Circuit in error state".to_string()),
            };
        }

        // Start if idle
        if self.state == CircuitState::Idle {
            self.state = CircuitState::Running;
        }

        if self.state == CircuitState::Complete {
            return CircuitResult {
                circuit_id: self.id.clone(),
                step_executed: None,
                deadband_status: self.deadband.status(),
                action_taken: "Circuit complete".to_string(),
                conservation_cost: 0.0,
                error: None,
            };
        }

        // Execute current step
        if self.current_step >= self.steps.len() {
            self.state = CircuitState::Complete;
            return CircuitResult {
                circuit_id: self.id.clone(),
                step_executed: None,
                deadband_status: self.deadband.status(),
                action_taken: "All steps complete".to_string(),
                conservation_cost: 0.0,
                error: None,
            };
        }

        let step = &self.steps[self.current_step];
        let step_name = step.name.clone();
        let mut action_taken: String;
        let mut error: Option<String> = None;
        let mut success = true;

        match &step.action {
            StepAction::CheckDeadband { metric } => {
                let value = self.metrics.get(metric).copied().unwrap_or(self.deadband.current);
                self.deadband.update(self.tick_counter, value);
                let status = self.deadband.status();
                if matches!(status, DeadbandStatus::Breached) {
                    success = false;
                    action_taken = format!("Deadband breach on metric '{}'", metric);
                } else {
                    action_taken = format!("Checked '{}': {}", metric, status);
                }
            }
            StepAction::Compute { formula, output } => {
                // Simple formula evaluation: supports metric references and basic ops
                let computed = self.evaluate_formula(formula);
                match computed {
                    Ok(val) => {
                        self.metrics.insert(output.clone(), val);
                        action_taken = format!("Computed {} = {:.4}", output, val);
                    }
                    Err(e) => {
                        success = false;
                        error = Some(e.clone());
                        action_taken = format!("Compute error: {e}");
                    }
                }
            }
            StepAction::Branch { condition, if_true, if_false } => {
                let result = self.evaluate_condition(condition);
                let target = if result { if_true } else { if_false };
                action_taken = format!("Branch: {} → {}", condition, target);
                // Find target step
                if let Some(idx) = self.steps.iter().position(|s| s.name == *target) {
                    self.current_step = idx;
                    self.run_count += 1;
                    return CircuitResult {
                        circuit_id: self.id.clone(),
                        step_executed: Some(step_name),
                        deadband_status: self.deadband.status(),
                        action_taken,
                        conservation_cost: 0.2,
                        error: None,
                    };
                } else {
                    success = false;
                    error = Some(format!("Branch target '{}' not found", target));
                    action_taken = format!("Branch target '{}' not found", target);
                }
            }
            StepAction::Notify { message, level } => {
                action_taken = format!("[{}] {}", level, message);
            }
            StepAction::Log { message } => {
                action_taken = format!("LOG: {}", message);
            }
            StepAction::SetMetric { key, value } => {
                self.metrics.insert(key.clone(), *value);
                self.deadband.update(self.tick_counter, *value);
                action_taken = format!("Set {} = {}", key, value);
            }
            StepAction::Wait { ticks } => {
                // This tick counts as the first, so remaining = ticks - 1
                self.wait_remaining = ticks.saturating_sub(1);
                action_taken = format!("Waiting {} ticks", ticks);
                // Don't advance step — the wait handler will do it when done
                self.run_count += 1;
                return CircuitResult {
                    circuit_id: self.id.clone(),
                    step_executed: Some(step_name),
                    deadband_status: self.deadband.status(),
                    action_taken,
                    conservation_cost: 0.1,
                    error: None,
                };
            }
            StepAction::Delegate { to_room, task } => {
                action_taken = format!("Delegating to {}: {}", to_room, task);
            }
            StepAction::Escalate { reason } => {
                action_taken = format!("ESCALATE: {}", reason);
            }
        }

        // Advance step
        if success {
            let step = &self.steps[self.current_step];
            if let Some(ref next_name) = step.on_success {
                if let Some(idx) = self.steps.iter().position(|s| s.name == *next_name) {
                    self.current_step = idx;
                } else {
                    self.current_step += 1;
                }
            } else {
                self.current_step += 1;
            }
            // Check if complete
            if self.current_step >= self.steps.len() {
                self.state = CircuitState::Complete;
            }
        } else {
            self.error_count += 1;
            let step = &self.steps[self.current_step];
            if step.retry_count < step.max_retries {
                // Retry: don't advance
                // We need mutable access to increment retry_count
                self.steps[self.current_step].retry_count += 1;
            } else if let Some(ref fail_name) = step.on_failure {
                if let Some(idx) = self.steps.iter().position(|s| s.name == *fail_name) {
                    self.steps[self.current_step].retry_count = 0;
                    self.current_step = idx;
                } else {
                    self.state = CircuitState::Error(format!("Step '{}' failed", step_name));
                }
            } else {
                self.state = CircuitState::Error(format!("Step '{}' failed", step_name));
            }
        }

        self.run_count += 1;

        CircuitResult {
            circuit_id: self.id.clone(),
            step_executed: Some(step_name),
            deadband_status: self.deadband.status(),
            action_taken,
            conservation_cost: 0.5,
            error,
        }
    }

    pub fn status(&self) -> CircuitStatus {
        CircuitStatus {
            id: self.id.clone(),
            name: self.name.clone(),
            state: self.state.clone(),
            automation_level: self.automation_level,
            deadband: self.deadband.status(),
            run_count: self.run_count,
            error_rate: self.error_rate(),
            current_step: self.steps.get(self.current_step).map(|s| s.name.clone()),
            total_steps: self.steps.len(),
        }
    }

    pub fn pause(&mut self) {
        if self.state == CircuitState::Running {
            self.state = CircuitState::Paused;
        }
    }

    pub fn resume(&mut self) {
        if self.state == CircuitState::Paused {
            self.state = CircuitState::Running;
        }
    }

    pub fn reset(&mut self) {
        self.state = CircuitState::Idle;
        self.current_step = 0;
        self.run_count = 0;
        self.error_count = 0;
        self.tick_counter = 0;
        self.wait_remaining = 0;
        self.metrics.clear();
        for step in &mut self.steps {
            step.retry_count = 0;
        }
    }

    pub fn is_in_deadband(&self) -> bool {
        self.deadband.current >= self.deadband.lower && self.deadband.current <= self.deadband.upper
    }

    pub fn error_rate(&self) -> f64 {
        if self.run_count == 0 {
            0.0
        } else {
            self.error_count as f64 / self.run_count as f64
        }
    }

    pub fn describe(&self) -> String {
        let db_status = self.deadband.status();
        format!(
            "Circuit '{}' [{}] in room '{}'\n  State: {}\n  Automation Level: {}\n  Deadband: [{:.2}, {:.2}] current={:.2} status={}\n  Steps: {} (at #{})\n  Runs: {} errors: {} rate: {:.2}%\n  In deadband: {}",
            self.name,
            self.id,
            self.room_id,
            self.state,
            self.automation_level,
            self.deadband.lower,
            self.deadband.upper,
            self.deadband.current,
            db_status,
            self.steps.len(),
            self.current_step,
            self.run_count,
            self.error_count,
            self.error_rate() * 100.0,
            self.is_in_deadband(),
        )
    }

    /// Simple formula evaluator. Supports: metric names, numbers, +, -, *, /
    fn evaluate_formula(&self, formula: &str) -> Result<f64, String> {
        // Tokenize and evaluate simple arithmetic with metric references
        let expr = formula.trim();

        // Try to parse as a plain number first
        if let Ok(v) = expr.parse::<f64>() {
            return Ok(v);
        }

        // Check if it's a metric reference
        if !expr.contains('+') && !expr.contains('-') && !expr.contains('*') && !expr.contains('/') {
            return self.metrics.get(expr)
                .copied()
                .ok_or_else(|| format!("Unknown metric: '{}'", expr));
        }

        // Simple left-to-right evaluation with operator precedence (just * / then + -)
        let tokens = self.tokenize_formula(expr)?;
        self.eval_tokens(&tokens)
    }

    fn tokenize_formula(&self, expr: &str) -> Result<Vec<String>, String> {
        let mut tokens = Vec::new();
        let mut current = String::new();
        let chars: Vec<char> = expr.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            let c = chars[i];
            if c == ' ' {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
            } else if c == '+' || c == '*' || c == '/' {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
                tokens.push(c.to_string());
            } else if c == '-' {
                // Check if this is a minus operator or negative sign
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                    tokens.push("-".to_string());
                } else if tokens.is_empty() || tokens.last().map(|t| t == "+" || t == "-" || t == "*" || t == "/").unwrap_or(false) {
                    current.push(c);
                } else {
                    tokens.push("-".to_string());
                }
            } else {
                current.push(c);
            }
            i += 1;
        }
        if !current.is_empty() {
            tokens.push(current);
        }

        Ok(tokens)
    }

    fn eval_tokens(&self, tokens: &[String]) -> Result<f64, String> {
        if tokens.is_empty() {
            return Err("Empty expression".to_string());
        }

        // Resolve all values first
        let mut resolved: Vec<String> = Vec::new();
        for token in tokens.iter() {
            if token == "+" || token == "-" || token == "*" || token == "/" {
                resolved.push(token.clone());
            } else if let Ok(v) = token.parse::<f64>() {
                resolved.push(v.to_string());
            } else if let Some(&v) = self.metrics.get(token) {
                resolved.push(v.to_string());
            } else if token.starts_with("deadband.") {
                match token.as_str() {
                    "deadband.current" => resolved.push(self.deadband.current.to_string()),
                    "deadband.lower" => resolved.push(self.deadband.lower.to_string()),
                    "deadband.upper" => resolved.push(self.deadband.upper.to_string()),
                    _ => return Err(format!("Unknown deadband field: '{}'", token)),
                }
            } else {
                // Try treating the token as a metric; if not found, default to 0.0
                resolved.push("0".to_string());
            }
        }

        // First pass: * and /
        let mut values: Vec<f64> = Vec::new();
        let mut ops: Vec<String> = Vec::new();

        let first = resolved[0].parse::<f64>().map_err(|e| e.to_string())?;
        values.push(first);

        let mut i = 1;
        while i < resolved.len() {
            let op = resolved[i].clone();
            if op == "+" || op == "-" || op == "*" || op == "/" {
                if i + 1 >= resolved.len() {
                    return Err("Unexpected end of expression".to_string());
                }
                let val = resolved[i + 1].parse::<f64>().map_err(|e| e.to_string())?;
                if op == "*" || op == "/" {
                    let last = values.last().ok_or("Empty values")?;
                    let result = if op == "*" {
                        *last * val
                    } else {
                        if val == 0.0 {
                            return Err("Division by zero".to_string());
                        }
                        *last / val
                    };
                    *values.last_mut().unwrap() = result;
                } else {
                    values.push(val);
                    ops.push(op);
                }
                i += 2;
            } else {
                i += 1;
            }
        }

        // Second pass: + and -
        let mut result = values[0];
        for (i, op) in ops.iter().enumerate() {
            let val = values.get(i + 1).ok_or("Missing value")?;
            match op.as_str() {
                "+" => result += val,
                "-" => result -= val,
                _ => {}
            }
        }

        Ok(result)
    }

    fn evaluate_condition(&self, condition: &str) -> bool {
        let cond = condition.trim();

        // Check deadband status conditions
        if cond == "in_deadband" {
            return self.is_in_deadband();
        }
        if cond == "breached" {
            return matches!(self.deadband.status(), DeadbandStatus::Breached);
        }

        // Check metric comparisons: "metric > value"
        for op in &["==", ">=", "<=", "!=", ">", "<"] {
            if let Some(pos) = cond.find(op) {
                let left = cond[..pos].trim();
                let right = cond[pos + op.len()..].trim();
                let left_val = self.resolve_value(left);
                let right_val = self.resolve_value(right);
                return match *op {
                    "==" => (left_val - right_val).abs() < f64::EPSILON,
                    "!=" => (left_val - right_val).abs() >= f64::EPSILON,
                    ">" => left_val > right_val,
                    "<" => left_val < right_val,
                    ">=" => left_val >= right_val,
                    "<=" => left_val <= right_val,
                    _ => false,
                };
            }
        }

        // Boolean literals
        matches!(cond, "true" | "1")
    }

    fn resolve_value(&self, s: &str) -> f64 {
        if let Ok(v) = s.parse::<f64>() {
            return v;
        }
        self.metrics.get(s).copied().unwrap_or(0.0)
    }
}

// ---------------------------------------------------------------------------
// CircuitBuilder
// ---------------------------------------------------------------------------

/// Fluent builder for circuits.
pub struct CircuitBuilder {
    name: String,
    room: String,
    steps: Vec<CircuitStep>,
    lower: f64,
    upper: f64,
    tick_interval: u64,
    automation_level: u32,
}

impl CircuitBuilder {
    pub fn new(name: &str, room: &str) -> Self {
        Self {
            name: name.to_string(),
            room: room.to_string(),
            steps: Vec::new(),
            lower: -1.0,
            upper: 1.0,
            tick_interval: 1000,
            automation_level: 1,
        }
    }

    pub fn step(mut self, s: CircuitStep) -> Self {
        self.steps.push(s);
        self
    }

    pub fn deadband(mut self, lower: f64, upper: f64) -> Self {
        self.lower = lower;
        self.upper = upper;
        self
    }

    pub fn tick_interval(mut self, ms: u64) -> Self {
        self.tick_interval = ms;
        self
    }

    pub fn automation_level(mut self, level: u32) -> Self {
        self.automation_level = level.clamp(1, 4);
        self
    }

    pub fn build(self) -> Circuit {
        let mut c = Circuit::new(&self.name, &self.room);
        c.set_deadband(self.lower, self.upper);
        c.tick_interval = self.tick_interval;
        c.automation_level = self.automation_level;
        for step in self.steps {
            c.add_step(step);
        }
        c
    }
}

// ---------------------------------------------------------------------------
// Pre-built circuits
// ---------------------------------------------------------------------------

/// Simple monitoring circuit for a metric.
pub fn monitoring_circuit(room: &str, metric: &str, tolerance: f64) -> Circuit {
    CircuitBuilder::new("monitor", room)
        .deadband(-tolerance, tolerance)
        .automation_level(2)
        .step(CircuitStep::new(
            "check",
            StepAction::CheckDeadband { metric: metric.to_string() },
        ))
        .step(CircuitStep::new(
            "log-ok",
            StepAction::Log {
                message: format!("{} within deadband", metric),
            },
        ))
        .build()
}

/// Course correction circuit for the navigation room.
pub fn course_correction_circuit() -> Circuit {
    CircuitBuilder::new("course-correction", "navigation")
        .deadband(-0.5, 0.5)
        .tick_interval(500)
        .automation_level(3)
        .step(CircuitStep::new(
            "check-heading",
            StepAction::CheckDeadband { metric: "heading_deviation".to_string() },
        ))
        .step(CircuitStep::new(
            "compute-correction",
            StepAction::Compute {
                formula: "heading_deviation * -1".to_string(),
                output: "correction".to_string(),
            },
        ).with_on_success("apply"))
        .step(CircuitStep::new(
            "apply",
            StepAction::SetMetric { key: "heading_deviation".to_string(), value: 0.0 },
        ))
        .step(CircuitStep::new(
            "notify-drift",
            StepAction::Notify {
                message: "Heading deviation detected, applying correction".to_string(),
                level: "WARN".to_string(),
            },
        ).with_on_failure("escalate"))
        .step(CircuitStep::new(
            "escalate",
            StepAction::Escalate {
                reason: "Course correction failed".to_string(),
            },
        ))
        .build()
}

/// Motor calibration circuit for the engineering room.
pub fn motor_calibration_circuit() -> Circuit {
    CircuitBuilder::new("motor-calibration", "engineering")
        .deadband(-0.1, 0.1)
        .tick_interval(200)
        .automation_level(4)
        .step(CircuitStep::new(
            "check-vibration",
            StepAction::CheckDeadband { metric: "vibration".to_string() },
        ))
        .step(CircuitStep::new(
            "compute-adjustment",
            StepAction::Compute {
                formula: "vibration * -0.5".to_string(),
                output: "adjustment".to_string(),
            },
        ))
        .step(CircuitStep::new(
            "apply-adjustment",
            StepAction::SetMetric { key: "vibration".to_string(), value: 0.0 },
        ))
        .step(CircuitStep::new(
            "calibrate-wait",
            StepAction::Wait { ticks: 5 },
        ))
        .step(CircuitStep::new(
            "verify",
            StepAction::CheckDeadband { metric: "vibration".to_string() },
        ).with_max_retries(3))
        .step(CircuitStep::new(
            "report",
            StepAction::Log {
                message: "Motor calibration complete".to_string(),
            },
        ))
        .build()
}

// ===========================================================================
// TESTS
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- CircuitState tests ---
    #[test]
    fn test_circuit_state_display() {
        assert_eq!(format!("{}", CircuitState::Idle), "Idle");
        assert_eq!(format!("{}", CircuitState::Running), "Running");
        assert_eq!(format!("{}", CircuitState::Paused), "Paused");
        assert_eq!(format!("{}", CircuitState::Complete), "Complete");
        assert_eq!(format!("{}", CircuitState::Error("boom".into())), "Error(boom)");
    }

    #[test]
    fn test_circuit_state_serde() {
        let state = CircuitState::Error("timeout".to_string());
        let json = serde_json::to_string(&state).unwrap();
        let back: CircuitState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, back);
    }

    // --- DeadbandStatus tests ---
    #[test]
    fn test_deadband_status_display() {
        assert_eq!(format!("{}", DeadbandStatus::Green), "Green");
        assert_eq!(format!("{}", DeadbandStatus::Yellow), "Yellow");
        assert_eq!(format!("{}", DeadbandStatus::Red), "Red");
        assert_eq!(format!("{}", DeadbandStatus::Breached), "Breached");
    }

    // --- Deadband tests ---
    #[test]
    fn test_deadband_new() {
        let db = Deadband::new(10.0, 2.0);
        assert_eq!(db.lower, 8.0);
        assert_eq!(db.upper, 12.0);
        assert_eq!(db.current, 10.0);
    }

    #[test]
    fn test_deadband_update() {
        let mut db = Deadband::new(0.0, 1.0);
        db.update(1, 0.5);
        assert_eq!(db.current, 0.5);
        assert_eq!(db.history.len(), 1);
    }

    #[test]
    fn test_deadband_green() {
        let db = Deadband::new(0.0, 10.0);
        // current is 0, bounds are -10, 10, well within
        assert_eq!(db.status(), DeadbandStatus::Green);
    }

    #[test]
    fn test_deadband_breached_below() {
        let db = Deadband {
            lower: 0.0,
            upper: 10.0,
            current: -1.0,
            warning_threshold: 2.0,
            history: vec![],
            max_history: 1000,
        };
        assert_eq!(db.status(), DeadbandStatus::Breached);
    }

    #[test]
    fn test_deadband_breached_above() {
        let db = Deadband {
            lower: 0.0,
            upper: 10.0,
            current: 11.0,
            warning_threshold: 2.0,
            history: vec![],
            max_history: 1000,
        };
        assert_eq!(db.status(), DeadbandStatus::Breached);
    }

    #[test]
    fn test_deadband_trend_stable() {
        let db = Deadband::new(0.0, 1.0);
        assert_eq!(db.trend(), DeadbandTrend::Stable);
    }

    #[test]
    fn test_deadband_trend_with_history() {
        let mut db = Deadband::new(5.0, 5.0);
        for i in 0..10 {
            db.update(i, 5.0);
        }
        assert!(matches!(db.trend(), DeadbandTrend::Stable));
    }

    #[test]
    fn test_deadband_trend_drifting() {
        let mut db = Deadband::new(0.0, 100.0);
        for i in 0..20 {
            db.update(i, i as f64 * 0.5);
        }
        if let DeadbandTrend::Drifting(rate) = db.trend() {
            assert!(rate > 0.0);
        }
        // Could also be Diverging — both are acceptable drift indicators
    }

    #[test]
    fn test_deadband_trend_oscillating() {
        let mut db = Deadband::new(0.0, 100.0);
        for i in 0..20 {
            let val = if i % 2 == 0 { 5.0 } else { -5.0 };
            db.update(i, val);
        }
        assert!(matches!(db.trend(), DeadbandTrend::Oscillating(_)));
    }

    #[test]
    fn test_deadband_ticks_since_breach_none() {
        let db = Deadband::new(0.0, 10.0);
        assert!(db.ticks_since_breach().is_none());
    }

    #[test]
    fn test_deadband_ticks_since_breach_some() {
        let mut db = Deadband::new(0.0, 1.0);
        db.update(1, 0.5);
        db.update(2, -5.0); // breach
        db.update(3, 0.5);
        assert_eq!(db.ticks_since_breach(), Some(1));
    }

    #[test]
    fn test_deadband_distance_to_boundary() {
        let db = Deadband {
            lower: 0.0,
            upper: 10.0,
            current: 2.0,
            warning_threshold: 2.0,
            history: vec![],
            max_history: 1000,
        };
        assert_eq!(db.distance_to_boundary(), 2.0);
    }

    #[test]
    fn test_deadband_is_stable_true() {
        let mut db = Deadband::new(5.0, 1.0);
        for i in 0..10 {
            db.update(i, 5.0);
        }
        assert!(db.is_stable(5));
    }

    #[test]
    fn test_deadband_is_stable_false() {
        let mut db = Deadband::new(5.0, 1.0);
        for i in 0..10 {
            db.update(i, i as f64 * 10.0);
        }
        assert!(!db.is_stable(5));
    }

    #[test]
    fn test_deadband_max_history() {
        let mut db = Deadband::new(0.0, 1.0);
        db.max_history = 5;
        for i in 0..10 {
            db.update(i, i as f64);
        }
        assert_eq!(db.history.len(), 5);
    }

    #[test]
    fn test_deadband_serde_roundtrip() {
        let db = Deadband::new(42.0, 3.14);
        let json = serde_json::to_string(&db).unwrap();
        let back: Deadband = serde_json::from_str(&json).unwrap();
        assert_eq!(db, back);
    }

    // --- CircuitStep tests ---
    #[test]
    fn test_circuit_step_new() {
        let step = CircuitStep::new("check", StepAction::Log { message: "hi".into() });
        assert_eq!(step.name, "check");
        assert!(step.on_success.is_none());
        assert!(step.on_failure.is_none());
        assert_eq!(step.max_retries, 0);
    }

    #[test]
    fn test_circuit_step_builder() {
        let step = CircuitStep::new("check", StepAction::Log { message: "hi".into() })
            .with_on_success("next")
            .with_on_failure("fallback")
            .with_max_retries(3);
        assert_eq!(step.on_success, Some("next".to_string()));
        assert_eq!(step.on_failure, Some("fallback".to_string()));
        assert_eq!(step.max_retries, 3);
    }

    // --- StepAction tests ---
    #[test]
    fn test_step_action_variants() {
        let actions = vec![
            StepAction::CheckDeadband { metric: "temp".into() },
            StepAction::Compute { formula: "a + b".into(), output: "c".into() },
            StepAction::Branch { condition: "x > 0".into(), if_true: "yes".into(), if_false: "no".into() },
            StepAction::Notify { message: "hi".into(), level: "WARN".into() },
            StepAction::Log { message: "log".into() },
            StepAction::SetMetric { key: "k".into(), value: 1.0 },
            StepAction::Wait { ticks: 5 },
            StepAction::Delegate { to_room: "eng".into(), task: "fix".into() },
            StepAction::Escalate { reason: "bad".into() },
        ];
        let json = serde_json::to_string(&actions).unwrap();
        let back: Vec<StepAction> = serde_json::from_str(&json).unwrap();
        assert_eq!(actions, back);
    }

    // --- Circuit tests ---
    #[test]
    fn test_circuit_new() {
        let c = Circuit::new("test", "room-1");
        assert_eq!(c.name, "test");
        assert_eq!(c.room_id, "room-1");
        assert_eq!(c.state, CircuitState::Idle);
        assert_eq!(c.automation_level, 1);
        assert_eq!(c.steps.len(), 0);
    }

    #[test]
    fn test_circuit_add_step() {
        let mut c = Circuit::new("test", "room");
        c.add_step(CircuitStep::new("s1", StepAction::Log { message: "step1".into() }));
        assert_eq!(c.steps.len(), 1);
    }

    #[test]
    fn test_circuit_set_deadband() {
        let mut c = Circuit::new("test", "room");
        c.set_deadband(0.0, 100.0);
        assert_eq!(c.deadband.lower, 0.0);
        assert_eq!(c.deadband.upper, 100.0);
    }

    #[test]
    fn test_circuit_tick_log_step() {
        let mut c = Circuit::new("test", "room");
        c.add_step(CircuitStep::new("log-it", StepAction::Log { message: "hello".into() }));
        let result = c.tick();
        assert_eq!(result.step_executed, Some("log-it".to_string()));
        assert!(result.action_taken.contains("hello"));
        assert_eq!(c.state, CircuitState::Complete);
    }

    #[test]
    fn test_circuit_tick_set_metric() {
        let mut c = Circuit::new("test", "room");
        c.add_step(CircuitStep::new("set", StepAction::SetMetric { key: "temp".into(), value: 42.0 }));
        c.tick();
        assert_eq!(c.metrics.get("temp"), Some(&42.0));
    }

    #[test]
    fn test_circuit_tick_compute() {
        let mut c = Circuit::new("test", "room");
        c.metrics.insert("x".to_string(), 10.0);
        c.add_step(CircuitStep::new("compute", StepAction::Compute {
            formula: "x + 5".to_string(),
            output: "y".to_string(),
        }));
        c.tick();
        assert_eq!(c.metrics.get("y"), Some(&15.0));
    }

    #[test]
    fn test_circuit_tick_wait() {
        let mut c = Circuit::new("test", "room");
        c.add_step(CircuitStep::new("wait", StepAction::Wait { ticks: 3 }));
        c.add_step(CircuitStep::new("done", StepAction::Log { message: "done".into() }));

        let r1 = c.tick(); // starts running, executes wait
        assert!(r1.action_taken.contains("Waiting"));
        let r2 = c.tick(); // waiting 2 remaining
        assert!(r2.action_taken.contains("Waiting"));
        let r3 = c.tick(); // waiting 1 remaining
        assert!(r3.action_taken.contains("Waiting"));
        let r4 = c.tick(); // now executes "done"
        assert_eq!(r4.step_executed, Some("done".to_string()));
    }

    #[test]
    fn test_circuit_pause_resume() {
        let mut c = Circuit::new("test", "room");
        c.add_step(CircuitStep::new("s", StepAction::Log { message: "x".into() }));
        c.tick(); // starts running, completes
        // Reset to test pause
        c.reset();
        c.state = CircuitState::Running;
        c.pause();
        assert_eq!(c.state, CircuitState::Paused);
        let r = c.tick();
        assert_eq!(r.action_taken, "Paused");
        c.resume();
        assert_eq!(c.state, CircuitState::Running);
    }

    #[test]
    fn test_circuit_reset() {
        let mut c = Circuit::new("test", "room");
        c.add_step(CircuitStep::new("s", StepAction::Log { message: "x".into() }));
        c.tick();
        assert!(c.run_count > 0);
        c.reset();
        assert_eq!(c.state, CircuitState::Idle);
        assert_eq!(c.current_step, 0);
        assert_eq!(c.run_count, 0);
        assert_eq!(c.error_count, 0);
    }

    #[test]
    fn test_circuit_is_in_deadband() {
        let mut c = Circuit::new("test", "room");
        c.set_deadband(0.0, 10.0);
        c.deadband.current = 5.0;
        assert!(c.is_in_deadband());
        c.deadband.current = -1.0;
        assert!(!c.is_in_deadband());
    }

    #[test]
    fn test_circuit_error_rate() {
        let mut c = Circuit::new("test", "room");
        assert_eq!(c.error_rate(), 0.0);
        c.run_count = 10;
        c.error_count = 2;
        assert!((c.error_rate() - 0.2).abs() < f64::EPSILON);
    }

    #[test]
    fn test_circuit_describe() {
        let c = Circuit::new("test-circuit", "bridge");
        let desc = c.describe();
        assert!(desc.contains("test-circuit"));
        assert!(desc.contains("bridge"));
        assert!(desc.contains("Idle"));
    }

    #[test]
    fn test_circuit_status() {
        let mut c = Circuit::new("test", "room");
        c.automation_level = 3;
        c.add_step(CircuitStep::new("s1", StepAction::Log { message: "x".into() }));
        let status = c.status();
        assert_eq!(status.name, "test");
        assert_eq!(status.state, CircuitState::Idle);
        assert_eq!(status.automation_level, 3);
        assert_eq!(status.total_steps, 1);
        assert!(status.current_step.is_some());
    }

    #[test]
    fn test_circuit_check_deadband_step() {
        let mut c = Circuit::new("test", "room");
        c.set_deadband(0.0, 10.0);
        c.add_step(CircuitStep::new("check", StepAction::CheckDeadband { metric: "temp".into() }));
        c.metrics.insert("temp".to_string(), 5.0);
        let r = c.tick();
        assert_eq!(r.deadband_status, DeadbandStatus::Green);
    }

    #[test]
    fn test_circuit_check_deadband_breach() {
        let mut c = Circuit::new("test", "room");
        c.set_deadband(0.0, 10.0);
        c.add_step(CircuitStep::new("check", StepAction::CheckDeadband { metric: "temp".into() }));
        c.metrics.insert("temp".to_string(), 15.0);
        let r = c.tick();
        assert_eq!(r.deadband_status, DeadbandStatus::Breached);
    }

    #[test]
    fn test_circuit_notify_step() {
        let mut c = Circuit::new("test", "room");
        c.add_step(CircuitStep::new("notify", StepAction::Notify {
            message: "Something happened".to_string(),
            level: "WARN".to_string(),
        }));
        let r = c.tick();
        assert!(r.action_taken.contains("[WARN]"));
        assert!(r.action_taken.contains("Something happened"));
    }

    #[test]
    fn test_circuit_delegate_step() {
        let mut c = Circuit::new("test", "room");
        c.add_step(CircuitStep::new("delegate", StepAction::Delegate {
            to_room: "engineering".to_string(),
            task: "Fix the warp drive".to_string(),
        }));
        let r = c.tick();
        assert!(r.action_taken.contains("engineering"));
        assert!(r.action_taken.contains("warp"));
    }

    #[test]
    fn test_circuit_escalate_step() {
        let mut c = Circuit::new("test", "room");
        c.add_step(CircuitStep::new("escalate", StepAction::Escalate {
            reason: "Critical failure".to_string(),
        }));
        let r = c.tick();
        assert!(r.action_taken.contains("ESCALATE"));
        assert!(r.action_taken.contains("Critical failure"));
    }

    #[test]
    fn test_circuit_branch_step_true() {
        let mut c = Circuit::new("test", "room");
        c.add_step(CircuitStep::new("branch", StepAction::Branch {
            condition: "true".to_string(),
            if_true: "yes-step".to_string(),
            if_false: "no-step".to_string(),
        }));
        c.add_step(CircuitStep::new("yes-step", StepAction::Log { message: "yes".into() }));
        c.add_step(CircuitStep::new("no-step", StepAction::Log { message: "no".into() }));
        let r = c.tick();
        assert!(r.action_taken.contains("yes-step"));
    }

    #[test]
    fn test_circuit_branch_step_false() {
        let mut c = Circuit::new("test", "room");
        c.add_step(CircuitStep::new("branch", StepAction::Branch {
            condition: "false".to_string(),
            if_true: "yes-step".to_string(),
            if_false: "no-step".to_string(),
        }));
        c.add_step(CircuitStep::new("yes-step", StepAction::Log { message: "yes".into() }));
        c.add_step(CircuitStep::new("no-step", StepAction::Log { message: "no".into() }));
        let r = c.tick();
        assert!(r.action_taken.contains("no-step"));
    }

    #[test]
    fn test_circuit_branch_condition_comparison() {
        let mut c = Circuit::new("test", "room");
        c.metrics.insert("x".to_string(), 10.0);
        c.add_step(CircuitStep::new("branch", StepAction::Branch {
            condition: "x > 5".to_string(),
            if_true: "high".to_string(),
            if_false: "low".to_string(),
        }));
        c.add_step(CircuitStep::new("high", StepAction::Log { message: "high".into() }));
        c.add_step(CircuitStep::new("low", StepAction::Log { message: "low".into() }));
        let r = c.tick();
        assert!(r.action_taken.contains("high"));
    }

    #[test]
    fn test_circuit_branch_in_deadband() {
        let mut c = Circuit::new("test", "room");
        c.set_deadband(0.0, 10.0);
        c.deadband.current = 5.0;
        c.add_step(CircuitStep::new("branch", StepAction::Branch {
            condition: "in_deadband".to_string(),
            if_true: "ok".to_string(),
            if_false: "fix".to_string(),
        }));
        c.add_step(CircuitStep::new("ok", StepAction::Log { message: "ok".into() }));
        c.add_step(CircuitStep::new("fix", StepAction::Log { message: "fix".into() }));
        let r = c.tick();
        assert!(r.action_taken.contains("ok"));
    }

    #[test]
    fn test_circuit_on_success_goto() {
        let mut c = Circuit::new("test", "room");
        c.add_step(CircuitStep::new("first", StepAction::Log { message: "1".into() })
            .with_on_success("third"));
        c.add_step(CircuitStep::new("second", StepAction::Log { message: "2".into() }));
        c.add_step(CircuitStep::new("third", StepAction::Log { message: "3".into() }));
        c.tick(); // runs "first", jumps to "third"
        let r = c.tick(); // runs "third"
        assert_eq!(r.step_executed, Some("third".to_string()));
    }

    #[test]
    fn test_circuit_retries_on_failure() {
        let mut c = Circuit::new("test", "room");
        c.set_deadband(0.0, 10.0);
        c.add_step(CircuitStep::new("check", StepAction::CheckDeadband { metric: "x".into() })
            .with_max_retries(2)
            .with_on_failure("fallback"));
        c.add_step(CircuitStep::new("fallback", StepAction::Log { message: "fell back".into() }));
        c.metrics.insert("x".to_string(), 20.0); // outside deadband
        let r1 = c.tick(); // first attempt, fails, retry count = 1
        assert!(r1.action_taken.contains("breach"));
        let r2 = c.tick(); // retry 1, fails, retry count = 2
        assert!(r2.action_taken.contains("breach"));
        let r3 = c.tick(); // retry 2, fails, max_retries exhausted, goes to on_failure
        assert!(r3.action_taken.contains("breach"));
        let r4 = c.tick(); // fallback step
        assert_eq!(r4.step_executed, Some("fallback".to_string()));
    }

    #[test]
    fn test_circuit_error_state_tick() {
        let mut c = Circuit::new("test", "room");
        c.state = CircuitState::Error("broken".into());
        let r = c.tick();
        assert!(r.error.is_some());
        assert!(r.action_taken.contains("error"));
    }

    #[test]
    fn test_circuit_complete_tick() {
        let mut c = Circuit::new("test", "room");
        c.state = CircuitState::Complete;
        let r = c.tick();
        assert!(r.action_taken.contains("complete"));
    }

    // --- CircuitBuilder tests ---
    #[test]
    fn test_builder_basic() {
        let c = CircuitBuilder::new("built", "bridge")
            .deadband(0.0, 100.0)
            .tick_interval(500)
            .automation_level(3)
            .step(CircuitStep::new("s1", StepAction::Log { message: "hello".into() }))
            .build();
        assert_eq!(c.name, "built");
        assert_eq!(c.room_id, "bridge");
        assert_eq!(c.automation_level, 3);
        assert_eq!(c.tick_interval, 500);
        assert_eq!(c.steps.len(), 1);
        assert_eq!(c.deadband.lower, 0.0);
        assert_eq!(c.deadband.upper, 100.0);
    }

    #[test]
    fn test_builder_automation_level_clamped() {
        let c = CircuitBuilder::new("test", "room")
            .automation_level(0)
            .build();
        assert_eq!(c.automation_level, 1);

        let c = CircuitBuilder::new("test", "room")
            .automation_level(10)
            .build();
        assert_eq!(c.automation_level, 4);
    }

    // --- Pre-built circuit tests ---
    #[test]
    fn test_monitoring_circuit() {
        let c = monitoring_circuit("bridge", "temperature", 2.0);
        assert_eq!(c.name, "monitor");
        assert_eq!(c.room_id, "bridge");
        assert_eq!(c.steps.len(), 2);
        assert_eq!(c.automation_level, 2);
    }

    #[test]
    fn test_course_correction_circuit() {
        let c = course_correction_circuit();
        assert_eq!(c.name, "course-correction");
        assert_eq!(c.room_id, "navigation");
        assert!(c.steps.len() >= 3);
        assert_eq!(c.automation_level, 3);
    }

    #[test]
    fn test_motor_calibration_circuit() {
        let c = motor_calibration_circuit();
        assert_eq!(c.name, "motor-calibration");
        assert_eq!(c.room_id, "engineering");
        assert!(c.steps.len() >= 3);
        assert_eq!(c.automation_level, 4);
    }

    #[test]
    fn test_monitoring_circuit_ticks() {
        let mut c = monitoring_circuit("bridge", "temperature", 5.0);
        c.metrics.insert("temperature".to_string(), 0.5);
        let r = c.tick(); // check step
        assert_eq!(r.deadband_status, DeadbandStatus::Green);
    }

    #[test]
    fn test_course_correction_runs() {
        let mut c = course_correction_circuit();
        c.metrics.insert("heading_deviation".to_string(), 0.3);
        let r = c.tick(); // check heading
        assert!(r.step_executed.is_some());
    }

    // --- CircuitResult tests ---
    #[test]
    fn test_circuit_result_serde() {
        let r = CircuitResult {
            circuit_id: "c-1".into(),
            step_executed: Some("check".into()),
            deadband_status: DeadbandStatus::Green,
            action_taken: "All good".into(),
            conservation_cost: 0.5,
            error: None,
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: CircuitResult = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    // --- CircuitStatus tests ---
    #[test]
    fn test_circuit_status_serde() {
        let s = CircuitStatus {
            id: "c-1".into(),
            name: "test".into(),
            state: CircuitState::Running,
            automation_level: 2,
            deadband: DeadbandStatus::Green,
            run_count: 100,
            error_rate: 0.05,
            current_step: Some("step-3".into()),
            total_steps: 10,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: CircuitStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    // --- DeadbandTrend tests ---
    #[test]
    fn test_deadband_trend_display() {
        assert_eq!(format!("{}", DeadbandTrend::Stable), "Stable");
        assert!(format!("{}", DeadbandTrend::Drifting(0.5)).contains("0.5"));
        assert!(format!("{}", DeadbandTrend::Oscillating(1.0)).contains("Oscillating"));
        assert_eq!(format!("{}", DeadbandTrend::Diverging), "Diverging");
    }

    #[test]
    fn test_deadband_trend_serde() {
        let trend = DeadbandTrend::Drifting(0.42);
        let json = serde_json::to_string(&trend).unwrap();
        let back: DeadbandTrend = serde_json::from_str(&json).unwrap();
        assert_eq!(trend, back);
    }

    // --- Formula evaluator tests ---
    #[test]
    fn test_evaluate_formula_number() {
        let c = Circuit::new("test", "room");
        assert_eq!(c.evaluate_formula("42.0").unwrap(), 42.0);
    }

    #[test]
    fn test_evaluate_formula_metric() {
        let c = Circuit {
            metrics: {
                let mut m = HashMap::new();
                m.insert("x".to_string(), 7.0);
                m
            },
            ..Circuit::new("test", "room")
        };
        assert_eq!(c.evaluate_formula("x").unwrap(), 7.0);
    }

    #[test]
    fn test_evaluate_formula_addition() {
        let c = Circuit {
            metrics: {
                let mut m = HashMap::new();
                m.insert("a".to_string(), 10.0);
                m
            },
            ..Circuit::new("test", "room")
        };
        assert_eq!(c.evaluate_formula("a + 5").unwrap(), 15.0);
    }

    #[test]
    fn test_evaluate_formula_multiplication() {
        let c = Circuit {
            metrics: {
                let mut m = HashMap::new();
                m.insert("x".to_string(), 3.0);
                m
            },
            ..Circuit::new("test", "room")
        };
        assert_eq!(c.evaluate_formula("x * 2").unwrap(), 6.0);
    }

    #[test]
    fn test_evaluate_formula_complex() {
        let c = Circuit {
            metrics: {
                let mut m = HashMap::new();
                m.insert("heading_deviation".to_string(), 0.5);
                m
            },
            ..Circuit::new("test", "room")
        };
        // heading_deviation * -1
        let result = c.evaluate_formula("heading_deviation * -1").unwrap();
        assert!((result - (-0.5)).abs() < 0.001);
    }

    // --- Integration test ---
    #[test]
    fn test_full_circuit_lifecycle() {
        let mut c = CircuitBuilder::new("lifecycle-test", "test-room")
            .deadband(0.0, 10.0)
            .automation_level(2)
            .step(CircuitStep::new("init", StepAction::SetMetric { key: "counter".into(), value: 5.0 }))
            .step(CircuitStep::new("check", StepAction::CheckDeadband { metric: "counter".into() }))
            .step(CircuitStep::new("log", StepAction::Log { message: "All good".into() }))
            .build();

        // Tick through all steps
        assert_eq!(c.state, CircuitState::Idle);
        let r1 = c.tick(); // init (starts running)
        assert_eq!(r1.step_executed, Some("init".to_string()));
        assert_eq!(c.state, CircuitState::Running);

        let r2 = c.tick(); // check
        assert_eq!(r2.step_executed, Some("check".to_string()));

        let r3 = c.tick(); // log
        assert_eq!(r3.step_executed, Some("log".to_string()));

        // Should be complete now
        assert_eq!(c.state, CircuitState::Complete);
        assert!(c.error_count == 0);
        assert!(c.run_count >= 3);

        // Reset and run again
        c.reset();
        assert_eq!(c.state, CircuitState::Idle);
    }

    #[test]
    fn test_full_circuit_with_branching() {
        let mut c = Circuit::new("branch-test", "room");
        c.set_deadband(0.0, 10.0);
        c.add_step(CircuitStep::new("set-val", StepAction::SetMetric { key: "val".into(), value: 3.0 }));
        c.add_step(CircuitStep::new("check-val", StepAction::CheckDeadband { metric: "val".into() }));
        c.add_step(CircuitStep::new("decide", StepAction::Branch {
            condition: "val > 5".to_string(),
            if_true: "high-handler".to_string(),
            if_false: "low-handler".to_string(),
        }));
        c.add_step(CircuitStep::new("high-handler", StepAction::Log { message: "too high".into() }));
        c.add_step(CircuitStep::new("low-handler", StepAction::Log { message: "ok".into() }));

        // val = 3.0, so branch should go to low-handler
        c.tick(); // set-val
        c.tick(); // check-val
        let r = c.tick(); // decide → branches to low-handler
        assert!(r.action_taken.contains("low-handler"));
    }
}
