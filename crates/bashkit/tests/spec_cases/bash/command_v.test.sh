# command -v and command -V tests
# Tests that command -v searches PATH for external scripts (issue #1120)

### command_v_finds_builtin
# command -v finds builtins
command -v echo
echo "exit=$?"
### expect
echo
exit=0
### end

### command_v_finds_function
# command -v finds functions
myfunc() { true; }
command -v myfunc
echo "exit=$?"
### expect
myfunc
exit=0
### end

### command_v_not_found
# command -v returns 1 for unknown commands
command -v nonexistent_cmd_xyz_12345
echo "exit=$?"
### expect
exit=1
### end

### command_v_searches_path
### skip: VFS-only test — real bash doesn't have /scripts on disk
# command -v finds executable scripts on PATH
mkdir -p /scripts
echo '#!/bin/bash' > /scripts/myscript
chmod +x /scripts/myscript
export PATH="/scripts:$PATH"
command -v myscript
echo "exit=$?"
### expect
/scripts/myscript
exit=0
### end

### command_V_builtin
# command -V describes builtins
command -V echo
### expect
echo is a shell builtin
### end

### command_V_path_script
### skip: VFS-only test — real bash doesn't have /scripts on disk
# command -V shows full path for scripts on PATH
mkdir -p /scripts
echo '#!/bin/bash' > /scripts/pathcmd
chmod +x /scripts/pathcmd
export PATH="/scripts:$PATH"
command -V pathcmd
### expect
pathcmd is /scripts/pathcmd
### end

### command_V_every_operand
# command -V describes every operand; status 0 when any was found
command -V printf nonexistent_cmd_xyz_12345 echo 2>/dev/null
echo "exit=$?"
command -V nonexistent_cmd_xyz_12345 2>&1
### expect
printf is a shell builtin
echo is a shell builtin
exit=0
bash: line 3: command: nonexistent_cmd_xyz_12345: not found
### end

### command_V_none_found
# command -V fails only when no operand was found
command -V nonexistent_cmd_xyz_12345 nonexistent_cmd_xyz_67890 2>/dev/null
echo "exit=$?"
### expect
exit=1
### end

### command_combined_flags
# -p, -v and -V combine like any getopt flags; the last of -v/-V wins
command -pv printf
command -vV printf
command -Vv printf
### expect
printf
printf is a shell builtin
printf
### end

### command_invalid_option
# An unknown flag is an invalid option, status 2
command -Z printf 2>&1
echo "exit=$?"
command -vZ printf 2>/dev/null
echo "exit=$?"
### expect
bash: line 1: command: -Z: invalid option
command: usage: command [-pVv] command [arg ...]
exit=2
exit=2
### end
