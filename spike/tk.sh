snap() { busctl call com.timekpr.server /com/timekpr/server com.timekpr.server.user.admin getUserInformation ss conrad F \
  | grep -oE '"(TIME_LEFT_DAY|TIME_SPENT_BALANCE|TIME_SPENT_DAY|LIMITS_PER_WEEKDAYS)" [a-z]+ [-0-9 ]+' | tr '\n' ' ' | sed 's/^/  /'; echo; }
stl() { printf 'setTimeLeft %s %s -> ' "$1" "$2"; runuser -u stcol -- busctl call com.timekpr.server /com/timekpr/server com.timekpr.server.user.admin setTimeLeft ssi conrad "$1" "$2"; }
