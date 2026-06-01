# lau-circuit

Deadband circuit system for Rust — self-contained automations that run within tolerance bands. When metrics stay inside the deadband, the circuit handles things. When they drift, you get notified. When they breach, the circuit escalates.

## What This Does

This crate provides a tick-based automation engine where each **circuit** is a sequence of typed steps that execute once per `tick()`. Steps can check metrics against a deadband, compute values with a simple expression evaluator, branch on conditions, wait, delegate, notify, and escalate. The deadband tracks history, status (Green / Yellow / Red / Breached), trend (Stable / Drifting / Oscillating / Diverging), and stability.

Everything is pure Rust with no I/O — circuits are data structures you drive from your own event loop. All state is `Serialize`/`Deserialize`, so you can persist circuits to JSON and restore them.

The crate includes three pre-built circuits (monitoring, course correction, motor calibration) and a fluent builder.

**66 tests.**

## Key Idea

A *deadband* is a tolerance interval `[lower, upper]`. The circuit continuously checks whether a metric value stays within that band. The automation only acts when the metric drifts close to or past a boundary. This minimizes unnecessary intervention — the circuit "runs itself" while everything is nominal, and surfaces alerts only when something changes.

## Install

```toml
[dependencies]
lau-circuit = "0.1.0"
```

Rust 2021 edition. Dependencies: `serde` + `serde_json`.

## Quick Start

### Build and run a circuit

```rust
use lau_circuit::*;

let mut circuit = CircuitBuilder::new("temp-monitor", "engineering")
    .deadband(18.0, 24.0)
    .automation_level(2)
    .step(CircuitStep::new("check", StepAction::CheckDeadband { metric: "temperature".into() }))
    .step(CircuitStep::new("log-ok", StepAction::Log { message: "Temperature nominal".into() }))
    .step(CircuitStep::new("alert", StepAction::Notify {
        message: "Temperature out of range!".into(),
        level: "WARN".into(),
    }).with_on_failure("escalate"))
    .step(CircuitStep::new("escalate", StepAction::Escalate {
        reason: "Temperature breach".into(),
    }))
    .build();

// Feed a metric value
circuit.metrics.insert("temperature".into(), 22.0);

// Tick the circuit forward
let result = circuit.tick();
println!("{}: {}", result.step_executed.unwrap_or("-".into()), result.action_taken);
```

### Use a pre-built circuit

```rust
use lau_circuit::*;

let mut monitor = monitoring_circuit("bridge", "temperature", 2.0);
monitor.metrics.insert("temperature".into(), 1.5);
let result = monitor.tick();
```

### Serialize and restore

```rust
let json = serde_json::to_string(&circuit).unwrap();
let restored: Circuit = serde_json::from_str(&json).unwrap();
```

## API Reference

### Core Types

| Type | Description |
|---|---|
| `Circuit` | The main automation engine. Holds state, steps, deadband, metrics. |
| `CircuitStep` | One step in the automation sequence. Has an action, optional success/failure routing, and retry count. |
| `CircuitBuilder` | Fluent builder for `Circuit`. |
| `Deadband` | Tolerance band with history, status, and trend tracking. |

### Circuit Lifecycle

| Method | Description |
|---|---|
| `Circuit::new(name, room_id)` | Create an empty circuit. |
| `circuit.add_step(step)` | Append a step. |
| `circuit.set_deadband(lower, upper)` | Configure the tolerance band. |
| `circuit.tick()` | Execute one step. Returns `CircuitResult`. |
| `circuit.pause()` / `circuit.resume()` | Pause/resume execution. |
| `circuit.reset()` | Return to idle, clear counters and metrics. |
| `circuit.status()` | Get a `CircuitStatus` snapshot. |
| `circuit.describe()` | Human-readable status string. |
| `circuit.is_in_deadband()` | Check current metric against band. |
| `circuit.error_rate()` | Errors / total ticks. |

### Step Actions (`StepAction`)

| Variant | Behavior |
|---|---|
| `CheckDeadband { metric }` | Read metric, update deadband, fail if breached. |
| `Compute { formula, output }` | Evaluate expression, store result in metrics. |
| `Branch { condition, if_true, if_false }` | Jump to named step based on condition. |
| `Notify { message, level }` | Emit a notification (logged in result). |
| `Log { message }` | Log a message. |
| `SetMetric { key, value }` | Set a metric value and update deadband. |
| `Wait { ticks }` | Stall for N ticks before advancing. |
| `Delegate { to_room, task }` | Mark delegation (logged in result). |
| `Escalate { reason }` | Mark escalation (logged in result). |

### Step Routing

```rust
CircuitStep::new("risky", StepAction::CheckDeadband { metric: "x".into() })
    .with_on_success("next-step")    // jump on success
    .with_on_failure("fallback")     // jump on failure
    .with_max_retries(3);            // retry before failing
```

### Deadband

| Method | Description |
|---|---|
| `Deadband::new(center, tolerance)` | Create band `[center - tol, center + tol]`. |
| `db.update(tick, value)` | Record a new value at the given tick. |
| `db.status()` | `Green`, `Yellow`, `Red`, or `Breached`. |
| `db.trend()` | `Stable`, `Drifting(rate)`, `Oscillating(amp)`, or `Diverging`. |
| `db.distance_to_boundary()` | Distance from current value to nearest edge. |
| `db.ticks_since_breach()` | Ticks since last breach, if any. |
| `db.is_stable(window)` | Whether variance over last N ticks is negligible. |

### Formula Evaluator

The `Compute` step evaluates simple arithmetic expressions:
- Number literals: `42.0`
- Metric references: `heading_deviation`
- Operators: `+`, `-`, `*`, `/` (with standard precedence)
- Special references: `deadband.current`, `deadband.lower`, `deadband.upper`

Examples: `"x + 5"`, `"heading_deviation * -1"`, `"a * 2 + b"`

### Condition Evaluator

The `Branch` step evaluates conditions:
- Boolean literals: `"true"`, `"false"`, `"1"`
- Named conditions: `"in_deadband"`, `"breached"`
- Comparisons: `"metric > 5"`, `"x <= 10"`, `"val == 3.14"`

### Pre-built Circuits

| Function | Purpose |
|---|---|
| `monitoring_circuit(room, metric, tolerance)` | Simple check + log loop. Automation level 2. |
| `course_correction_circuit()` | Heading deviation check, compute correction, apply, notify, escalate. Level 3. |
| `motor_calibration_circuit()` | Vibration check, compute adjustment, wait, verify (with retries), report. Level 4. |

## How It Works

### Tick Execution Model

A circuit is a state machine. Each call to `tick()` executes one step and advances the internal step pointer. The circuit starts `Idle`, transitions to `Running` on the first tick, and ends at `Complete` when all steps are exhausted (or `Error` if a step fails without a fallback).

Steps can redirect flow:
- **`on_success`** / **`on_failure`** links jump to named steps.
- **`Branch`** conditionally jumps.
- **`Wait`** stalls for N ticks.
- **Retries**: if `max_retries > 0`, a failing step is retried before following `on_failure`.

### Deadband Status Calculation

Status is based on the current value's distance to the boundaries:
- **Breached**: outside `[lower, upper]`
- **Red**: inside but close to boundary (margin > warning threshold)
- **Yellow**: approaching boundary
- **Green**: safely within band

### Trend Detection

Trend analysis uses the last 10 history points:
1. Count sign changes in successive deltas. If >60% alternate, classify as **Oscillating**.
2. Compute linear drift rate. If negligible, classify as **Stable**.
3. Compare drift rates in first and second halves of history. If second half is >1.5× the first, classify as **Diverging**.
4. Otherwise, classify as **Drifting(rate)**.

### Expression Evaluation

The formula evaluator tokenizes the input, resolves metric references to their numeric values, then evaluates with operator precedence (multiplication/division first, then addition/subtraction). It's not a full expression parser — no parentheses, no functions — but handles the common cases for metric arithmetic.

## The Math

The deadband is simply a tolerance interval `[center - ε, center + ε]`. Status levels partition this interval into zones:

```
Breached  |  Red  |  Yellow  |  Green  |  Yellow  |  Red  |  Breached
          lower                                   upper
```

The warning threshold (default: 80% of tolerance) sets the boundary between Green and Yellow.

Trend drift rate is computed as:

$$\text{rate} = \frac{x_n - x_0}{n}$$

Divergence is detected by comparing drift in the first half vs second half of the observation window:

$$\text{diverging if } |r_{\text{second}}| > 1.5 \cdot |r_{\text{first}}|$$

## License

MIT
