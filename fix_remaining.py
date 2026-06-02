#!/usr/bin/env python3
"""Fix remaining compilation errors by line number in src/tui/mod.rs."""
import sys

PATH = '/home/synth/projects/openshark/src/tui/mod.rs'
with open(PATH, 'r') as f:
    lines = f.readlines()

changes = 0

# Fix 1: Line 2494 - Err(e) => {  →  Ok(Err(e)) => {
if '                    Err(e) => {\n' in lines[2493]:
    lines[2493] = '                    Ok(Err(e)) => {\n'
    # Insert timeout arm after line 2497 (the closing } of this Err arm)
    # Line 2494-2497 is: Err => { send error; send Done; }
    # After fixing err to Ok(Err), we need to add Err(_) arm before line 2498's }
    indent = '                    '
    lines.insert(2497, f'{indent}Err(_) => {{\n')
    lines.insert(2498, f'{indent}    let _ = tx.send(StreamEvent::Error("Follow-up timed out after 120s".to_string()));\n')
    lines.insert(2499, f'{indent}    let _ = tx.send(StreamEvent::Done);\n')
    lines.insert(2500, f'{indent}}}\n')
    changes += 1
    print("Fix 1 (line 2494): Err -> Ok(Err) + timeout arm")
else:
    print(f"Fix 1 FAILED at line 2494: {repr(lines[2493])}")

# Fix 2: Line 2528 + offset from above insertions (now +4 lines)
# The original line 2528 is now at 2532
target_idx = None
for i, line in enumerate(lines):
    stripped = line.strip()
    if stripped == 'Err(e) => {' and 'Retry follow-up failed' in (lines[i+1] if i+1 < len(lines) else ''):
        if i > 2490:  # second occurrence
            target_idx = i
            break

if target_idx is not None:
    indent = lines[target_idx][:len(lines[target_idx]) - len(lines[target_idx].lstrip())]
    lines[target_idx] = indent + 'Ok(Err(e)) => {\n'
    # Find the end of this arm (matching })
    depth = 0
    arm_end = target_idx
    for j in range(target_idx + 1, len(lines)):
        for ch in lines[j]:
            if ch == '{': depth += 1
            elif ch == '}': depth -= 1
        if depth == 0:
            arm_end = j
            break
    lines.insert(arm_end, f'{indent}Err(_) => {{\n')
    lines.insert(arm_end + 1, f'{indent}    let _ = tx.send(StreamEvent::Error("Follow-up timed out after 120s".to_string()));\n')
    lines.insert(arm_end + 2, f'{indent}    let _ = tx.send(StreamEvent::Done);\n')
    lines.insert(arm_end + 3, f'{indent}}}\n')
    changes += 1
    print(f"Fix 2 (line {target_idx+1}): Err -> Ok(Err) + timeout arm")
else:
    print("Fix 2 FAILED: second Err(e) pattern not found")

# Fix 3 & 4: Lines 2855 and 2859 (now shifted), and 3072 and 3076 (shifted)
# These are the over-matched retry_req calls in other functions
# They need: Ok((x, _m)) -> Ok(Ok((x, _m))) and Err -> Ok(Err) + timeout

# Find all remaining Err(e) => patterns near timeout lines
for i in range(len(lines)):
    if lines[i].strip() == 'Err(e) => {' and i > 2500:
        # Check if there's a timeout line above
        has_timeout = False
        for j in range(max(0, i-20), i):
            if 'tokio::time::timeout' in lines[j]:
                has_timeout = True
                break
        if has_timeout:
            indent = lines[i][:len(lines[i]) - len(lines[i].lstrip())]
            lines[i] = indent + 'Ok(Err(e)) => {\n'
            # Add timeout arm
            depth = 0
            arm_end = i
            for j in range(i + 1, len(lines)):
                for ch in lines[j]:
                    if ch == '{': depth += 1
                    elif ch == '}': depth -= 1
                if depth == 0:
                    arm_end = j
                    break
            lines.insert(arm_end, f'{indent}Err(_) => {{\n')
            lines.insert(arm_end + 1, f'{indent}    let _ = tx.send(StreamEvent::Error("Follow-up timed out after 120s".to_string()));\n')
            lines.insert(arm_end + 2, f'{indent}    let _ = tx.send(StreamEvent::Done);\n')
            lines.insert(arm_end + 3, f'{indent}}}\n')
            changes += 1
            print(f"Fix (line {i+1}+): Err -> Ok(Err) + timeout arm")

# Fix Ok patterns that need Ok(Ok) conversion
for i in range(len(lines)):
    stripped = lines[i].strip()
    if stripped.startswith('Ok((') and '_metrics)' in stripped and 'tokio' not in stripped:
        # Check if timeout context above
        has_timeout = False
        for j in range(max(0, i-20), i):
            if 'tokio::time::timeout' in lines[j]:
                has_timeout = True
                break
        if has_timeout and 'Ok(Ok(' not in stripped:
            # Convert Ok((...) to Ok(Ok((...)
            old = lines[i].strip()
            new = old.replace('Ok((', 'Ok(Ok((', 1)
            if new != old:
                lines[i] = lines[i][:len(lines[i]) - len(lines[i].lstrip())] + new + '\n'
                changes += 1
                print(f"Fix (line {i+1}): Ok -> Ok(Ok): {old[:60]}")

# Write
with open(PATH, 'w') as f:
    f.writelines(lines)

print(f"\nTotal fixes applied: {changes}")
