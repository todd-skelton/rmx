#!/bin/sh
# Instructions per request for each router config in the router_profile example: callgrind
# runs it with 1,000 and with 5,000 requests and the difference is divided by 4,000, so setup
# drops out. Run inside rsmixbench-profile (see profile.Dockerfile).
set -e
for config in "handler" "4 pass-through" "4 async" "bench stack"; do
  small=$(valgrind --tool=callgrind --callgrind-out-file=/tmp/small.out /router_profile "$config" 1000 2>&1 | sed -n 's/.*Collected : *//p')
  large=$(valgrind --tool=callgrind --callgrind-out-file=/tmp/large.out /router_profile "$config" 5000 2>&1 | sed -n 's/.*Collected : *//p')
  echo "$config: $(( (large - small) / 4000 )) instructions/request"
  if [ -n "$TOP" ]; then
    callgrind_annotate --inclusive=no /tmp/large.out 2>/dev/null | sed -n '1,200p' | grep -E '^ *[0-9,]+ ' | head -n "$TOP"
  fi
done
