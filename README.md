# Overview

This is an AI agent that takes in a directory of a git repository and reviews the code changes to ensure forward compatibility of that repository.

# Strategy
1. initialization
2. collect git diff
3. prompt construction
4. Send to LLM
5. Output

# Usage
execute `cargo run [directory]`, where [directory] points to the git repository you want to analyze.

# Roadmap
- [ ] Make output streaming
- [ ] Stress test with large chunk of diff
- [ ] Content splitting / compression when diff is too large
- [ ] Provide more tools to LLM and support multi-round (let LLM run tests, inspect files, get project structure overview, etc.)

# Examples
- example output for breaking changes:

```
diff --git a/kernel/net.c b/kernel/net.c
index a7eead6..2bb3fc2 100644
--- a/kernel/net.c
+++ b/kernel/net.c
@@ -74,7 +74,7 @@ sys_bind(void)
 
   // find free slot
   int freei = -1;
-  for(int i = 0; i < MAX_BOUND_PORTS; i++){
+  for(int i = 0; i <= MAX_BOUND_PORTS; i++){
     if(!bound_ports[i].inuse){
       freei = i;
       break;
@@ -91,7 +91,7 @@ sys_bind(void)
   bound_ports[freei].head = bound_ports[freei].tail = bound_ports[freei].count = 0;
 
   release(&netlock);
-  return 0;
+  return freei; // Return index instead of 0
 }
 
 
@@ -181,6 +181,11 @@ sys_recv(void)
   while(bound_ports[idx].count == 0){
     sleep(&bound_ports[idx], &netlock);
     // wakeup will re-acquire netlock automatically when sleep returns
+    // Check if port is still bound after waking up
+    if(!bound_ports[idx].inuse || bound_ports[idx].port != dport){
+      release(&netlock);
+      return -1;
+    }
   }
 
   // pop packet
@@ -382,7 +387,7 @@ ip_rx(char *buf, int len)
   }
 
   // if queue full -> drop (but do not affect other ports)
-  if(bound_ports[idx].count >= MAX_PKT_QUEUE){
+  if(bound_ports[idx].count > MAX_PKT_QUEUE){
     release(&netlock);
     kfree(buf);
     return;

```
```
🚀 Starting code review for: ../xv6-labs-2024
🔍 Validating git repository...
📊 Getting unstaged changes...
📝 Found 1 modified file(s):
  - kernel/net.c

🤖 Analyzing changes with AI...
🔍 CODE REVIEW ANALYSIS
================================================================================
## Summary
The changes in `kernel/net.c` introduce **multiple breaking changes** that can destabilize the network subsystem. The most critical issues are an **off-by-one buffer overflow** in `sys_bind`, a **semantic change in the return value of `sys_bind`**, and a **queue-full check that now silently drops one extra packet**. These changes can lead to memory corruption, application-level failures, and dropped packets without notification.

---

## Detailed Analysis

### File: kernel/net.c

#### 1. Loop bound change in `sys_bind`
- **Breaking Change**: **Yes**
- **Severity**: **High**
- **Impact**:  
  The loop condition changed from `i < MAX_BOUND_PORTS` to `i <= MAX_BOUND_PORTS`.  
  `bound_ports` is declared as an array of size `MAX_BOUND_PORTS`, so index `MAX_BOUND_PORTS` is **out of bounds**. Writing to `bound_ports[MAX_BOUND_PORTS]` corrupts adjacent memory (likely the next global variable or kernel stack), leading to undefined behavior, crashes, or exploitable vulnerabilities.
- **Suggestions**:  
  Revert the loop bound to `i < MAX_BOUND_PORTS`. If the intent was to allow one more port, increase `MAX_BOUND_PORTS` by 1 instead.

---

#### 2. Return value change in `sys_bind`
- **Breaking Change**: **Yes**
- **Severity**: **Medium**
- **Impact**:  
  Previously, `sys_bind` always returned `0` on success. Now it returns the index `freei` (0-based). Applications that checked `bind(...) == 0` will still work, but any code that assumed the return value was always `0` (e.g., treating non-zero as error) will break. Additionally, returning the index leaks internal kernel state, which may be undesirable.
- **Suggestions**:  
  - If the index is needed internally, store it in a separate out-parameter or global.  
  - Keep the public return value as `0` on success and `-1` on error to maintain backward compatibility.  
  - Document the change clearly if the new behavior is intentional.

---

#### 3. Port-unbound check in `sys_recv`
- **Breaking Change**: **Yes**
- **Severity**: **Medium**
- **Impact**:  
  After waking from `sleep`, the code now checks if the port is still bound (`!bound_ports[idx].inuse || bound_ports[idx].port != dport`). If not, it returns `-1`. Previously, `sys_recv` would block indefinitely or crash if the port was unbound while waiting. Applications that relied on the old behavior (e.g., expecting to block forever) will now receive `-1` and must handle it. This is a **behavioral change**, not a bug fix, and could break existing code that doesn’t expect early return.
- **Suggestions**:  
  - Document this change as a **fix** rather than a breaking change, but provide a migration guide.  
  - Optionally add a flag (e.g., `SOCK_NONBLOCK`) to let applications opt into the new behavior.

---

#### 4. Queue-full check in `ip_rx`
- **Breaking Change**: **Yes**
- **Severity**: **Low**
- **Impact**:  
  The condition changed from `>= MAX_PKT_QUEUE` to `> MAX_PKT_QUEUE`. This means the queue can now hold `MAX_PKT_QUEUE + 1` packets instead of `MAX_PKT_QUEUE`. While this might seem like a minor fix, it silently changes the queue capacity. If applications or tests depend on the exact queue size (e.g., for flow control), they may behave differently.
- **Suggestions**:  
  - Revert to `>= MAX_PKT_QUEUE` or explicitly increase `MAX_PKT_QUEUE` by 1 in the definition.  
  - Document the change if the new capacity is intentional.

---

### Priority Fixes
1. **Fix the buffer overflow in `sys_bind` immediately** (loop bound).  
2. **Clarify the return value contract** for `sys_bind`.  
3. **Document the behavioral change in `sys_recv`**.  
4. **Align the queue-full check with the intended capacity**.
================================================================================

✅ Code review completed!
```

- example output for this safe change:
```
diff --git a/kernel/buf.h b/kernel/buf.h
index 4616e9e..b0c3a6c 100644
--- a/kernel/buf.h
+++ b/kernel/buf.h
@@ -7,6 +7,6 @@ struct buf {
   uint refcnt;
   struct buf *prev; // LRU cache list
   struct buf *next;
-  uchar data[BSIZE];
+  uchar data[BSIZE]; // buffer data
 };
```
```
🤖 Analyzing changes with AI...
🔍 CODE REVIEW ANALYSIS
================================================================================
## Summary
The provided git diff does not indicate a breaking change as it only adds a comment to an existing structure without altering the structure's definition or behavior.

## Detailed Analysis
### File: kernel/buf.h
- **Breaking Change**: No
- **Severity**: Low
- **Impact**: The addition of a comment does not change the structure of the `buf` struct or the behavior of the code that uses it. It simply provides more information to developers about the purpose of the `data` field.
- **Suggestions**: Since this is not a breaking change, no specific action is needed to prevent or mitigate this change. However, it's always good practice to ensure that comments are accurate and up-to-date to maintain code readability and maintainability.
================================================================================

✅ Code review completed!
```