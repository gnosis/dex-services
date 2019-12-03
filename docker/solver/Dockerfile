FROM python:3.6-slim-stretch as COMPILE

RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates wget make g++ zlib1g-dev bison flex libgmp-dev libreadline-dev libncurses5-dev \
  && rm -rf /var/lib/apt/lists/*

# Install SCIP solver
WORKDIR /opt
RUN wget https://scip.zib.de/download/release/scipoptsuite-6.0.1.tgz \
  && tar xzf scipoptsuite-6.0.1.tgz \
  && cd scipoptsuite-6.0.1 \
  && make \
  && cd scip/interfaces/ampl/ \
  && ./get.ASL \
  && cd solvers \
  && sh configurehere && make \
  && cd .. &&  make scipampl && make \
  && cp bin/scipampl /usr/local/bin/

RUN python -m venv /opt/venv
# Make sure we use the virtualenv:
ENV PATH="/opt/venv/bin:$PATH"

ENV GUROBI_HOME /opt/gurobi810/linux64/
ENV PATH ${PATH}:${GUROBI_HOME}/bin
ENV LD_LIBRARY_PATH ${LD_LIBRARY_PATH}:${GUROBI_HOME}/lib

RUN wget https://packages.gurobi.com/8.1/gurobi8.1.0_linux64.tar.gz \
  && tar xzf gurobi8.1.0_linux64.tar.gz \
  && rm gurobi8.1.0_linux64.tar.gz \
  && cd gurobi810/linux64/ \
  && python setup.py install \
  && ls | grep -v lib | xargs rm -r

# Multi Stage Build, since all we need is the scipampl binary from before and the gurobi packages
FROM python:3.6-slim-stretch
COPY --from=COMPILE /opt/scipoptsuite-6.0.1/scip/interfaces/ampl/bin/scipampl /usr/local/bin/
COPY --from=COMPILE /opt/venv /opt/venv
ENV PATH="/opt/venv/bin:$PATH"