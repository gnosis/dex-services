## Docker Landscape

There are four main solver images supporting this project:
1. Solver (SCIP dependencies for linear optimization)
2. Rust Base (based on Solver)
2. Rust - driver, listener, stableX (based on Rust Base)
3. Truffle (optional, migrations can be run from host system)

The reason for having a Rust Base image is mostly to improve CI performance. 
Downloading and building the rust workspace from scratch takes a lot of time. 
We therefore created a base image that has the majority of the dependencies pre-installed.
Travis still builds the rust image on every run but has then finds most dependencies in the build cache.
It is advisable to update this image from time to time to keep build times minimal.


## Updating Docker Images

To update the base image, run from the top level git repository:

```sh
docker build --tag gnosispm/dfusion-rust-base:vX --file docker/rust/base/Dockerfile.

docker push gnosispm/dfusion-rust-base:vX
```

This will create and publish a new version of the rust-base-image (version X) to Dockerhub.

In the dependent docker file it can the be used like this:

```docker
FROM gnosispm/dfusion-rust-base:vX
```

The solver image dependency can be updated in a similar manner.