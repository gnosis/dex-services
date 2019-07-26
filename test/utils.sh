RED="\033[1;31m"
GREEN="\033[1;32m"
NOCOLOR="\033[0m"

CHECKMARK="\xE2\x9C\x94"
CROSS="\xE2\x9D\x8C"

query_graphql() {
    echo "{ \"query\": \"" > query
    echo ${1//\"/\\\"} >> query
    echo "\" }" >> query
    result=$(curl -s \
        -X POST \
        -H "Content-Type: application/json" \
        --data @query \
        http://localhost:8000/subgraphs/name/dfusion)
    rm query
    echo $result
}
# Takes two arguments
# 1) a string describing the step
# 2) the command to execute
step() {
    with_backoff "$1" "$2" 0 0
}

# Same as above, but retries up to 5 times
step_with_retry() {
    with_backoff "$1" "$2" 5 .3
}

# Takes four arguments
# $1 - description of the command
# $2 - command to be executed
# $3 - remaining retries
# $4 - backoff time
function with_backoff {
    local description=$1
    local cmd=$2
    local remaining=$3
    local backoff=$4

    unset e
    
    bash -c "$cmd" &> output.txt
    if [[ $? == 0 ]]
    then
      echo -e "$GREEN$CHECKMARK $description $NOCOLOR"
      rm output.txt
    else
        if [[ $remaining == 0 ]]
        then
            echo -e "$RED$CROSS $1 $NOCOLOR"
            echo "Command: $cmd"
            cat output.txt
            rm output.txt
            set -e
            false
        else
            sleep $backoff
            backoff=$(node -pe $4*2)
            with_backoff "$description" "$cmd" $(( remaining - 1 )) $backoff
        fi
    fi

}