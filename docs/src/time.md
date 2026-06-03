# How time is measured

## Wall-clock time

Real elapsed time a project occupied your day. Computed by merging all overlapping turn intervals across sessions, then summing the non-overlapping ranges.

Five parallel 1-minute sessions = **1 minute** wall-clock.

## Accumulated time

Total agent-hours invested. Sum of all individual turn effective durations across all sessions.

Five parallel 1-minute sessions = **5 minutes** accumulated.

## Effective duration

Each turn is charged:

```
if gap to next turn < buffer * 2:
    effective = gap + agent_thinking_time
else:
    effective = buffer + agent_thinking_time + buffer
```

The default buffer is 5 minutes each side — time budgeted for writing the prompt and reading the response. If you re-prompt within 10 minutes, the actual gap is used instead of double-counting the buffer.
