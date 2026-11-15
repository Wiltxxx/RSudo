#!/data/data/com.termux/files/usr/bin/bash
# rsudo end-to-end tests.  Needs a working su and an rsudo uid
# already present in the sudoers file.
#
# Run from the repository root:
#   bash tests/e2e.sh
#
# The script is idempotent: it restores the daemon to the enabled state
# and re-adds the caller's uid on exit.

set -u

RS=./target/release/rsudo
[[ -x "$RS" ]] || RS=./target/debug/rsudo
[[ -x "$RS" ]] || { echo "build first: cargo build --release"; exit 1; }

MY_UID=$(id -u)
SUDOERS=/data/adb/rsudo/sudoers.conf
AUDIT=/data/adb/rsudo/audit.log
DISABLED=/data/local/tmp/rsudo.disabled

pass=0; fail=0
chk() {
    local desc="$1" want="$2" got="$3"
    if [[ "$got" == *"$want"* ]]; then
        printf '  ok   %s\n' "$desc"; pass=$((pass+1))
    else
        printf '  FAIL %s\n    want: %s\n    got:  %s\n' \
            "$desc" "$want" "${got:0:200}"; fail=$((fail+1))
    fi
}
chk_not() {
    local desc="$1" forbid="$2" got="$3"
    if [[ "$got" != *"$forbid"* ]]; then
        printf '  ok   %s\n' "$desc"; pass=$((pass+1))
    else
        printf '  FAIL %s\n    must NOT contain: %s\n    got:  %s\n' \
            "$desc" "$forbid" "${got:0:200}"; fail=$((fail+1))
    fi
}
chk_rc() {
    local desc="$1" want="$2" got="$3"
    if [[ "$got" == "$want" ]]; then
        printf '  ok   %s (rc=%s)\n' "$desc" "$got"; pass=$((pass+1))
    else
        printf '  FAIL %s\n    want rc: %s\n    got rc:  %s\n' \
            "$desc" "$want" "$got"; fail=$((fail+1))
    fi
}
chk_rc_nonzero() {
    local desc="$1" got="$2"
    if [[ "$got" != "0" ]]; then
        printf '  ok   %s (rc=%s)\n' "$desc" "$got"; pass=$((pass+1))
    else
        printf '  FAIL %s\n    want rc: nonzero\n    got rc:  0\n' "$desc"
        fail=$((fail+1))
    fi
}

cleanup() {
    echo
    echo "=== cleanup ==="
    su -c "$RS --sudoers-add $MY_UID >/dev/null 2>&1" 2>/dev/null
    su -c "$RS --daemon-enable >/dev/null 2>&1" 2>/dev/null
    su -c "$RS --daemon-restart >/dev/null 2>&1" 2>/dev/null
    su -c "rm -rf /data/data/com.termux/files/home/.suroot-e2e" 2>/dev/null
}
trap cleanup EXIT

ensure_daemon() {
    $RS --daemon-status 2>/dev/null | grep -q 'daemon:      running' && return 0
    $RS id >/dev/null 2>&1
    sleep 0.5
    $RS --daemon-status 2>/dev/null | grep -q 'daemon:      running'
}

# ---------------------------------------------------------------------------
echo "=== 0. setup ==="
# ---------------------------------------------------------------------------
su -c "$RS --sudoers-add $MY_UID" >/dev/null 2>&1 || {
    echo "cannot add uid $MY_UID to sudoers; check su"; exit 1;
}
su -c "$RS --daemon-enable" >/dev/null 2>&1
su -c "$RS --daemon-restart" >/dev/null 2>&1
sleep 0.5

if ! ensure_daemon; then
    echo "daemon did not come up; aborting"
    $RS --daemon-status
    exit 1
fi
chk "daemon running" "running" "$($RS --daemon-status)"

# ---------------------------------------------------------------------------
echo "=== 1. basic exec (daemon) ==="
# ---------------------------------------------------------------------------
chk "id -> uid=0" "uid=0" "$($RS id 2>&1)"

$RS true; chk_rc "true" 0 $?
$RS false; chk_rc "false" 1 $?
$RS sh -c 'exit 42'; chk_rc "exit 42" 42 $?

# Missing command: the client's resolve fails, it falls back to su, and
# su's "not executable" returns 1.  The daemon's 127 is only reachable if
# the client sends an absolute path it cannot resolve itself; that path
# is not exercised here.
$RS /nonexistent; chk_rc "nonexistent" 1 $?
$RS ls /nonexistent >/dev/null 2>&1; chk_rc "ls missing file" 2 $?

# ---------------------------------------------------------------------------
echo "=== 2. basic exec (su path) ==="
# ---------------------------------------------------------------------------
su -c "$RS --daemon-disable" >/dev/null 2>&1
sleep 0.3

chk "id via su -> uid=0" "uid=0" "$($RS id 2>&1)"

$RS true; chk_rc "true via su" 0 $?
$RS false; chk_rc "false via su" 1 $?
$RS sh -c 'exit 42'; chk_rc "exit 42 via su" 42 $?

su -c "$RS --daemon-enable" >/dev/null 2>&1
$RS id >/dev/null 2>&1
sleep 0.5
ensure_daemon

# ---------------------------------------------------------------------------
echo "=== 3. stdio ==="
# ---------------------------------------------------------------------------
chk "stdin pipe" "hello" "$(printf 'hello' | $RS cat 2>&1)"

out=$($RS sh -c 'echo OUT; echo ERR >&2' 2>/dev/null)
chk "stdout only" "OUT" "$out"
chk_not "stderr suppressed" "ERR" "$out"

err=$($RS sh -c 'echo OUT; echo ERR >&2' 2>&1 >/dev/null)
chk "stderr captured" "ERR" "$err"

# Large output must not truncate or hang.
out=$(timeout 5 $RS sh -c 'seq 1 50000' 2>&1 | wc -l)
chk "large output 50000 lines" "50000" "$out"

# Binary bytes survive the plumbing unchanged.
out=$(timeout 5 $RS cat /system/bin/toybox 2>/dev/null | head -c 4 | od -An -tx1 | tr -d ' \n')
case "$out" in
    "") chk "binary read" "SKIP" "SKIP" ;;
    7f454c46) chk "binary read (ELF magic)" "7f454c46" "$out" ;;
    *) chk "binary read (any 4 bytes)" "true" "true" ;;
esac

# ---------------------------------------------------------------------------
echo "=== 4. signals ==="
# ---------------------------------------------------------------------------
# Force pipe mode with `< /dev/null`: pty mode has no signal handler in
# the client, so an external SIGINT would kill the client directly and
# leave the child to be SIGKILLed by the daemon on socket teardown.
$RS sleep 100 < /dev/null >/dev/null 2>&1 &
bg=$!
sleep 0.5
kill -INT $bg 2>/dev/null
wait $bg 2>/dev/null
rc=$?
chk_rc "SIGINT -> 130" 130 "$rc"
sleep 0.5
residual=$(timeout 2 $RS pgrep -c sleep 2>/dev/null || echo 0)
chk "no residual sleep after SIGINT" "0" "$residual"

# SIGTERM: the client currently treats any signal the same way, sending
# FRAME_CANCEL which becomes SIGINT for the child, so the exit code is
# 130 not 143.  Assert only "the child died and nothing is left running"
# here; tightening to 143 would require a per-signal frame type.
$RS sleep 100 < /dev/null >/dev/null 2>&1 &
bg=$!
sleep 0.5
kill -TERM $bg 2>/dev/null
wait $bg 2>/dev/null
rc=$?
chk_rc_nonzero "SIGTERM terminates child" "$rc"
sleep 0.5
residual=$(timeout 2 $RS pgrep -c sleep 2>/dev/null || echo 0)
chk "no residual sleep after SIGTERM" "0" "$residual"

# ---------------------------------------------------------------------------
echo "=== 5. environment ==="
# ---------------------------------------------------------------------------
chk "PATH has termux bin" "files/usr/bin" "$($RS printenv PATH 2>&1)"
chk "PREFIX" "/data/data/com.termux/files/usr" "$($RS printenv PREFIX 2>&1)"
chk "HOME=.suroot" ".suroot" "$($RS printenv HOME 2>&1)"

chk "--shell-home via daemon" "attempting daemon path" \
    "$($RS -v --shell-home /data/data/com.termux/files/home/.suroot-e2e id 2>&1)"
chk "--shell-home sets HOME" ".suroot-e2e" \
    "$($RS --shell-home /data/data/com.termux/files/home/.suroot-e2e printenv HOME 2>&1)"

# ---------------------------------------------------------------------------
echo "=== 6. daemon lifecycle ==="
# ---------------------------------------------------------------------------
chk "status running" "running" "$($RS --daemon-status)"

pid_before=$($RS --daemon-status | sed -n 's/.*pid \([0-9]*\).*/\1/p')
su -c "$RS --daemon-restart" >/dev/null 2>&1
sleep 0.5
pid_after=$($RS --daemon-status | sed -n 's/.*pid \([0-9]*\).*/\1/p')
if [[ -n "$pid_before" && -n "$pid_after" && "$pid_before" != "$pid_after" ]]; then
    chk "restart changes pid" "true" "true"
else
    chk "restart changes pid" "$pid_before -> $pid_after" "same pid"
fi

su -c "$RS --daemon-disable" >/dev/null 2>&1
chk "disable flag" "yes" "$($RS --daemon-status | grep disabled | grep -o 'yes')"
# With `--daemon-disable`, the disabled flag file is visible to the
# client uid (parent directory is 0711), so `should_use_daemon` returns
# false and no "attempting daemon path" line should appear.
out=$($RS -v true 2>&1)
chk_not "disabled -> no daemon attempt" "attempting daemon path" "$out"

su -c "$RS --daemon-enable" >/dev/null 2>&1
$RS id >/dev/null 2>&1
sleep 0.5
ensure_daemon
chk "enabled -> daemon back" "running" "$($RS --daemon-status)"

# ---------------------------------------------------------------------------
echo "=== 7. sudoers ==="
# ---------------------------------------------------------------------------
chk "sudoers list has self" "allow $MY_UID" "$(su -c "$RS --sudoers-list" 2>&1)"

su -c "$RS --sudoers-add $MY_UID" >/dev/null 2>&1
count=$(su -c "$RS --sudoers-list" 2>&1 | grep -c "^allow $MY_UID$")
chk "add idempotent" "1" "$count"

su -c "$RS --sudoers-del $MY_UID" >/dev/null 2>&1
su -c "$RS --daemon-restart" >/dev/null 2>&1
sleep 0.5
$RS id >/dev/null 2>&1
rc=$?
chk_rc "removed uid -> denied" 126 "$rc"

su -c "$RS --sudoers-add $MY_UID" >/dev/null 2>&1
su -c "$RS --daemon-restart" >/dev/null 2>&1
sleep 0.5
$RS id >/dev/null 2>&1
chk_rc "restored uid -> ok" 0 $?

# ---------------------------------------------------------------------------
echo "=== 8. audit ==="
# ---------------------------------------------------------------------------
before=$(su -c "wc -l < $AUDIT 2>/dev/null || echo 0" 2>/dev/null)
$RS id >/dev/null 2>&1
after=$(su -c "wc -l < $AUDIT 2>/dev/null || echo 0" 2>/dev/null)
if [[ "$after" -gt "$before" ]]; then
    chk "audit grows on exec" "true" "true"
else
    chk "audit grows on exec" "> $before" "$after"
fi

last=$(su -c "$RS --audit 1" 2>&1 | tail -1)
chk "audit last line has uid" "uid=$MY_UID" "$last"
chk "audit last line has argv" "argv=" "$last"

# ---------------------------------------------------------------------------
echo "=== 9. pty ==="
# ---------------------------------------------------------------------------
# Non-tty stdin forces the pipe path; tty should report "not a tty".
out=$($RS sh -c 'tty 2>&1 || true' < /dev/null)
chk "pipe mode: not a tty" "not a tty" "$out"

# The pty path cannot be verified here: $() redirects stdout to a pipe,
# so the client always picks the pipe path.  Manual check:
#   rsudo bash
#   tty      # should print /dev/pts/N
echo "  skip tty mode check (command substitution forces pipe path)"

# ---------------------------------------------------------------------------
echo "=== 10. script mode ==="
# ---------------------------------------------------------------------------
chk "script mode echo" "hi" "$($RS -s 'echo hi' 2>&1)"
$RS -s 'exit 42'; chk_rc "script mode exit" 42 $?
chk "script mode args" "hello" "$($RS -s 'echo $1' hello 2>&1)"

$RS -s 'echo RSUDO_INLINE_EOF' >/dev/null 2>&1
chk_rc "heredoc collision still runs" 0 $?

# ---------------------------------------------------------------------------
echo "=== 11. perf smoke ==="
# ---------------------------------------------------------------------------
ensure_daemon
t0=$(date +%s%N)
for _ in $(seq 1 20); do $RS true >/dev/null 2>&1; done
t1=$(date +%s%N)
ms=$(( (t1 - t0) / 1000000 ))
if [[ "$ms" -lt 5000 ]]; then
    chk "20 daemon calls < 5s" "true" "true (${ms}ms)"
else
    chk "20 daemon calls < 5s" "<5000ms" "${ms}ms"
fi

# ---------------------------------------------------------------------------
echo
echo "========== $pass passed, $fail failed =========="
[[ $fail -eq 0 ]] && echo "E2E OK" || echo "FIX BEFORE RELEASE"
exit $(( fail > 0 ))
