query_graphql() {
    curl -s \
        -X POST \
        -H "Content-Type: application/json" \
        --data "{ \"query\": \" $1 \" }" \
        http://localhost:8000/subgraphs/name/dfusion
}