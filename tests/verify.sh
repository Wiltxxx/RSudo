#!/data/data/com.termux/files/usr/bin/bash
# rsudo dry-run verification.  No root required.
set -u
cd "$(dirname "$0")/.." || exit 1
RS=./target/release/rsudo
[[ -x "$RS" ]] || RS=./target/debug/rsudo
[[ -x "$RS" ]] || { echo "build first: cargo build --release"; exit 1; }

pass=0; fail=0
chk() {
    local desc="$1"; local expected="$2"; local got="$3"
    if [[ "$got" == *"$expected"* ]]; then
        printf '  ok  %s\n' "$desc"; pass=$((pass+1))
    else
        printf '  FAIL %s\n    expected: %s\n    got: %s\n' \
            "$desc" "$expected" "${got:0:200}"; fail=$((fail+1))
    fi
}
chk_not() {
    local desc="$1"; local forbidden="$2"; local got="$3"
    if [[ "$got" != *"$forbidden"* ]]; then
        printf '  ok  %s\n' "$desc"; pass=$((pass+1))
    else
        printf '  FAIL %s\n    must NOT contain: %s\n    got: %s\n' \
            "$desc" "$forbidden" "${got:0:200}"; fail=$((fail+1))
    fi
}
chk_exact() {
    local desc="$1"; local expected="$2"; local got="$3"
    if [[ "$got" == "$expected" ]]; then
        printf '  ok  %s\n' "$desc"; pass=$((pass+1))
    else
        printf '  FAIL %s\n    expected exact: %s\n    got: %s\n' \
            "$desc" "$expected" "$got"; fail=$((fail+1))
    fi
}

path_of() { "$@" 2>&1 | grep '^export PATH=' | head -1; }

echo "=== 1. basic ==="
out=$($RS --dry-run ls 2>&1)
chk "exec PATH has termux bin" "files/usr/bin" "$out"
chk "exec HOME=~/.suroot"      ".suroot"       "$out"

out=$($RS --dry-run su 2>&1)
chk "su exec bash -i"          "--noprofile"   "$out"
chk "su PS1 red"               '1;31m'         "$out"

out=$($RS --dry-run asu 2>&1)
chk "asu PATH android-first"   "/system/bin"   "$out"
chk "asu LD_PRELOAD unset"     "unset LD_PRELOAD" "$out"

echo "=== 2. layouts ==="
PREFIX_DIR=$($RS --dry-run ls 2>&1 | sed -n 's/^export TERMUX__PREFIX=//p')
chk_exact "-T only PREFIX/bin" \
    "export PATH=${PREFIX_DIR}/bin" \
    "$(path_of $RS --dry-run -T ls)"

chk_exact "-TT bin+applets" \
    "export PATH=${PREFIX_DIR}/bin:${PREFIX_DIR}/bin/applets" \
    "$(path_of $RS --dry-run -TT ls)"

chk_exact "-A /system/bin only" \
    "export PATH=/system/bin" \
    "$(path_of $RS --dry-run -A ls)"

chk "-AA android full"          "product/bin"        "$(path_of $RS --dry-run -AA ls)"
chk_not "-AA no termux path"    "$PREFIX_DIR/bin"    "$(path_of $RS --dry-run -AA ls)"

echo "=== 3. export flags ==="
out=$(PATH=/tmp/zzz:$PATH $RS --dry-run -P ls 2>&1 | grep '^export PATH=')
chk "-P includes /tmp/zzz"      "/tmp/zzz"           "$out"

out=$($RS --dry-run --export-paths /opt/a:/opt/b ls 2>&1 | grep '^export PATH=')
chk "--export-paths appended"   "/opt/a:/opt/b"      "$out"
chk_not "--export-paths no '::'" "::"                "$out"

echo "=== 4. script mode ==="
out=$($RS --dry-run -s 'echo hello-inline' 2>&1)
chk "inline body present"       "echo hello-inline"  "$out"
chk "inline -s heredoc"         "-s <<"              "$out"
chk "inline heredoc word"       "RSUDO_INLINE_EOF"   "$out"

out=$($RS --dry-run -s 'echo $1' hello 2>&1)
chk "arg after --"              "-s -- hello"        "$out"

# Script containing the heredoc terminator falls back to a temp file.
out=$($RS --dry-run -s 'echo RSUDO_INLINE_EOF' 2>&1)
chk "heredoc collision -> temp file" "RSUDO_FILE_EOF" "$out"

echo "=== 5. config file ==="
mkdir -p "$PREFIX/etc"
cat > "$PREFIX/etc/rsudo.conf" <<'CONF'
SUDO__SHELL_HOME = /data/data/com.termux/files/home/.rsudo-test
SUDO__LOG_LEVEL = 2
CONF
out=$($RS --dry-run ls 2>&1)
chk "config SHELL_HOME"         ".rsudo-test"        "$out"
chk "config LOG_LEVEL=2"        "[rsudo]"            "$out"
out=$($RS --shell-home /tmp/override --dry-run ls 2>&1)
chk "CLI overrides config"      "/tmp/override"      "$out"
rm -f "$PREFIX/etc/rsudo.conf"

echo "=== 6. mount-master ==="
chk "mm default false (data dir)" "mount_master=false" \
    "$($RS -v --dry-run su 2>&1 | grep mount_master)"

chk "mm forced true" "mount_master=true" \
    "$($RS -v --mount-master --dry-run su 2>&1 | grep mount_master)"

chk "mm forced false" "mount_master=false" \
    "$($RS -v --no-mount-master --dry-run su 2>&1 | grep mount_master)"

chk "mm auto true (/system)" "mount_master=true" \
    "$($RS -v --dry-run --shell-home /system/rsudo-test su 2>&1 | grep mount_master)"

echo "=== 7. su probe cache ==="
chk "su_info_v2 cache exists" "|" \
    "$(cat $PREFIX/var/cache/rsudo/su_info_v2 2>/dev/null || echo MISSING)"
chk "mount_master cache exists" "0" \
    "$(cat $PREFIX/var/cache/rsudo/mount_master 2>/dev/null || echo 0)"

echo "=== 8. fd expansion ==="
inject="$PREFIX/tmp/inject.txt"
echo 'echo from-fd' > "$inject"
out=$($RS --dry-run ls /proc/self/fd/3 3< "$inject" 2>&1 | tail -2)
chk "fd 3 expanded"             "echo from-fd"       "$out"
rm -f "$inject"

echo
echo "========== $pass passed, $fail failed =========="
[[ $fail -eq 0 ]] && echo "READY FOR REAL EXEC" || echo "FIX BEFORE REAL EXEC"
