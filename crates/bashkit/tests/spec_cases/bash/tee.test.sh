### tee_basic
# tee writes to file and stdout
echo "hello" | tee /tmp/tee_out.txt
### expect
hello
### end

### tee_file_contents
# tee creates file with correct contents
echo "tee data" | tee /tmp/tee_check.txt > /dev/null
cat /tmp/tee_check.txt
### expect
tee data
### end

### tee_append
# tee -a appends to file
echo "first" > /tmp/tee_append.txt
echo "second" | tee -a /tmp/tee_append.txt > /dev/null
cat /tmp/tee_append.txt
### expect
first
second
### end

### tee_multiple_files
# tee writes to multiple files
echo "multi" | tee /tmp/tee_m1.txt /tmp/tee_m2.txt > /dev/null
cat /tmp/tee_m1.txt
cat /tmp/tee_m2.txt
### expect
multi
multi
### end

### tee_overwrite
# tee overwrites existing file by default
echo "old" > /tmp/tee_ow.txt
echo "new" | tee /tmp/tee_ow.txt > /dev/null
cat /tmp/tee_ow.txt
### expect
new
### end

### tee_multiline
# tee handles multiline input
printf "line1\nline2\nline3\n" | tee /tmp/tee_ml.txt > /dev/null
cat /tmp/tee_ml.txt
### expect
line1
line2
line3
### end

### tee_unknown_flag_rejected
### bash_diff: clap-backed tee returns exit 2 for parse errors; GNU tee returns 1
# Unknown short flag is rejected with a usage error
echo x | tee -z 2>/dev/null; echo "exit=$?"
### expect
exit=2
### end

### tee_ignore_interrupts_accepted
# -i is accepted (no-op in bashkit's virtual mode — no signals)
echo "ok" | tee -i /tmp/tee_i.txt > /dev/null
cat /tmp/tee_i.txt
### expect
ok
### end

### tee_reports_a_file_it_cannot_write_and_keeps_going
# GNU tee: the input still reaches stdout and the other files; status 1.
mkdir -p /tmp/tee_dir
echo "kept" | tee /tmp/tee_dir /tmp/tee_after.txt; echo "status $?"
cat /tmp/tee_after.txt
m=$( { echo x | tee /tmp/tee_dir >/dev/null; } 2>&1 ); echo "$m"
### expect
kept
status 1
kept
tee: /tmp/tee_dir: Is a directory
### end
