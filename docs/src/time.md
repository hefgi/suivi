# How time is measured

## Wall-clock time

Attention time: each completed turn becomes an interval
`[started − buffer, ended + buffer]`; overlapping intervals across all
sessions are merged and the non-overlapping ranges summed.

Five parallel 1-minute sessions = **1 minute** wall-clock.

## Agent time

Machine effort: the sum of each completed turn's real duration
(prompt submitted → response finished).

Five parallel 1-minute sessions = **5 minutes** of agent time — summing
across parallel sessions is intentional, because it measures what the agents
did rather than what you attended to.

## Effective duration

Each turn also stores an *engaged-time* estimate (`effective_duration_secs`),
combining gap-corrected human think time with agent time:

```
if gap_to_next_turn < buffer × 2:
    effective = gap + agent_duration
else:
    effective = buffer + agent_duration + buffer
```

The default buffer is **5 minutes** each side — time budgeted for writing the
prompt and reading the response. If you re-prompt within 10 minutes
(2 × buffer), the actual gap is used instead.

Effective duration is not currently shown in the headline views (wall-clock
and agent time are), but it is stored and included in JSON/CSV exports.

Configure the buffer in `~/.config/suivi/config.toml`:

```toml
[tracking]
human_buffer_secs = 300   # default: 5 minutes each side
```

## Interrupted turns

If an agent run never produces a Stop event (you interrupted it, or it
crashed), the next prompt in the same session closes the turn at that
moment, charging the elapsed time.

## Implementation detail

`suivi hook stop` derives the agent duration from the turn's own timestamps
(no agent sends a duration field) and writes the best-guess effective
duration immediately. When `suivi hook pre` fires for the next prompt in the
same session, it corrects the previous turn's effective duration if the gap
was shorter than `2 × buffer`. No background process required.
