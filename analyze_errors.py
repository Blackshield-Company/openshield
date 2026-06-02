#!/usr/bin/env python3
"""Fix remaining compilation errors in src/tui/mod.rs:
- Lines 2488, 2522: Err(e) -> Ok(Err(e)) + Err(_) timeout
- Lines 2855, 3072: Ok -> Ok(Ok) and Err -> Ok(Err) for over-matched retry_req calls
"""
import sys

PATH = '/home/synth/projects/openshark/src/tui/mod.rs'
with open(PATH, 'r') as f:
    lines = f.readlines()

changes = 0

# Show every Err(e) => that follows a tokio::time::timeout line
print("=== Finding Err(e) patterns near timeout ===")
for i, line in enumerate(lines):
    if 'Err(e) =>' in line:
        # Check if there's a timeout line within the last 10 lines
        has_timeout = False
        for j in range(max(0, i-10), i):
            if 'tokio::time::timeout' in lines[j]:
                has_timeout = True
                break
        if has_timeout:
            print(f"Line {i+1}: {line.rstrip()}")
            # Show context
            for j in range(max(0,i-1), min(len(lines),i+4)):
                print(f"  {j+1}: {lines[j].rstrip()}")
            print()

print("=== Finding mismatched Ok patterns ===")
for i, line in enumerate(lines):
    if 'mismatched types' not in line:
        continue
# Actually let me just look for all Ok patterns inside timeout blocks
for i, line in enumerate(lines):
    if line.strip().startswith('Ok((') and 'retry_chunks' in line:
        has_timeout = False
        for j in range(max(0, i-10), i):
            if 'tokio::time::timeout' in lines[j]:
                has_timeout = True
                break
        if has_timeout:
            print(f"Line {i+1}: {line.rstrip()} [HAS timeout above, should be Ok(Ok(...))]")
        
PYEOF
print("Analysis complete")
