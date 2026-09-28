#!/usr/bin/env bash
set -euo pipefail
awk 'BEGIN{skip=0;depth=0} /^    #\[cfg\(test\)\]$/{skip=1;next} skip==1{ if($0 ~ /^    mod tests \{/){depth=1;next} if(depth>0){ n=gsub(/\{/,"{"); m=gsub(/\}/,"}"); depth+=n-m; if(depth<=0){skip=0} next} } {print}' | grep -v '^[[:space:]]*$'
