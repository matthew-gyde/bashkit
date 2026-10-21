### funcnest_overrun_keeps_output_and_resumes_next_line
# Past FUNCNEST the call abandons its line; earlier output is kept and the
# next line runs with status 1.
FUNCNEST=8
echo before
f() { f; }
f
echo "after $?"
### expect
before
after 1
### end

### funcnest_overrun_discards_rest_of_line
# Like bash's DISCARD: the rest of the line, handlers included, is skipped.
FUNCNEST=8
f() { f; }
f; echo same-line
f || echo handled
echo "next $?"
### expect
next 1
### end

### funcnest_overrun_ends_subshell_substitution_and_pipeline_stage
FUNCNEST=8
f() { f; }
( f ); echo "subshell $?"
x=$(f; echo unreached); echo "substitution $? [$x]"
f | cat; echo "pipeline $? ${PIPESTATUS[*]}"
### expect
subshell 1
substitution 1 []
pipeline 0 1 0
### end

### funcnest_overrun_discards_compound_commands
FUNCNEST=8
f() { f; }
if f; then echo yes; else echo no; fi
echo "after if $?"
for i in 1 2; do f; echo "loop $i"; done
echo "after loop $?"
{
  echo in-group
  f
  echo unreached
}
echo "after group $?"
### expect
after if 1
after loop 1
in-group
after group 1
### end

### funcnest_allows_exactly_the_limit
FUNCNEST=8
g() { if [ "$1" -gt 0 ]; then g $(( $1 - 1 )); else echo bottom; fi; }
g 7; echo "seven $?"
g 8; echo unreached
echo "eight $?"
### expect
bottom
seven 0
eight 1
### end

### funcnest_overrun_unwinds_callers_and_eval
FUNCNEST=3
f() { f; }
h() { echo h-start; f; echo h-end; }
h; echo unreached
echo "after h $?"
eval 'f; echo in-eval'; echo "after eval $?"
### expect
h-start
after h 1
after eval 1
### end

### funcnest_overrun_does_not_trigger_errexit
set -e
FUNCNEST=4
f() { f; }
f
echo "continues $?"
### expect
continues 1
### end

### funcnest_ignores_non_positive_and_non_numeric_values
g() { if [ "$1" -gt 0 ]; then g $(( $1 - 1 )); else echo bottom; fi; }
FUNCNEST=0; g 5; echo "zero $?"
FUNCNEST=abc; g 5; echo "text $?"
FUNCNEST=-3; g 5; echo "negative $?"
FUNCNEST=' 3 '; g 2; echo "spaced $?"
FUNCNEST=' 3 '; g 3; echo unreached
echo "spaced over $?"
### expect
bottom
zero 0
bottom
text 0
bottom
negative 0
bottom
spaced 0
spaced over 1
### end

### funcnest_overrun_message
FUNCNEST=2
f() { f; }
m=$( { f; } 2>&1 ); echo "${m#*line *: }"
### expect
f: maximum function nesting level exceeded (2)
### end
