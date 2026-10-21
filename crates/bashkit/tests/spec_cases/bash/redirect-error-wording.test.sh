### redirect_onto_a_directory_fails
### exit_code: 1
mkdir d
echo x > d 2>/dev/null
### end

### trailing_slash_target_creates_nothing
echo y > nodir/ 2>/dev/null
echo "rc=$?"
ls
### expect
rc=1
### end

### append_to_a_trailing_slash_fails
echo z >> nodir/ 2>/dev/null
echo "rc=$?"
### expect
rc=1
### end

### redirect_failure_reports_through_earlier_stderr_redirect
# Redirects open left to right: a failed one reports through fd 2 as the
# redirects before it left it.
mkdir -p /tmp/rd
m=$( { echo x 2>/dev/null > /tmp/rd; } 2>&1 ); echo "devnull first [$m] $?"
m=$( { echo x > /tmp/rd 2>/dev/null; } 2>&1 ); echo "devnull after [${m#*: line *: }]"
m=$( { echo x 2>/dev/null >> /tmp/rd; } 2>&1 ); echo "append [$m]"
{ echo x > /tmp/rd; } 2>/dev/null || echo "group $?"
echo x 2>/dev/null > /tmp/rd || echo "handled $?"
### expect
devnull first [] 1
devnull after [/tmp/rd: Is a directory]
append []
group 1
handled 1
### end

### redirect_failure_reports_to_an_earlier_stderr_file
mkdir -p /tmp/rd
echo x 2>/tmp/rd.err > /tmp/rd; echo "status $?"
sed 's/^.*line [0-9]*: //' /tmp/rd.err
### expect
status 1
/tmp/rd: Is a directory
### end

### redirect_failure_follows_stderr_dup_order
mkdir -p /tmp/rd
m=$( { echo x 2>&1 > /tmp/rd; } 2>/dev/null ); echo "dup first [${m#*: line *: }]"
m=$( { echo x > /tmp/rd 2>&1; } 2>&1 >/dev/null ); echo "dup after [${m#*: line *: }]"
### expect
dup first [/tmp/rd: Is a directory]
dup after [/tmp/rd: Is a directory]
### end
