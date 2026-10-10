---
name: automation
description: Build repeatable local jobs or service integrations with dry runs, idempotency, and observable outcomes.
---

# Automation workflow

Define the trigger, inputs, side effects, and stopping conditions. Implement a dry run when actions mutate external state. Address duplicate execution, bounded retries, cancellation, and recovery according to the actual risk. Keep credentials outside source and logs. Exercise success, failure, and repeated execution with deterministic fixtures before live actions. Identify which integrations are simulated and which were called live. Record exact acceptance evidence. A script is not a scheduled job until the scheduler is configured and verified. External messages, purchases, and publication require the user's authorization for that action. Save clear run and recovery instructions.
