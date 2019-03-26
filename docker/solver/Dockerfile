FROM python:3.6

RUN apt-get update \
    && apt-get install -y --no-install-recommends g++ zlib1g-dev bison flex libgmp-dev libreadline-dev libncurses5-dev

# Install SCIP solver
WORKDIR /opt
RUN wget https://scip.zib.de/download/release/scipoptsuite-6.0.1.tgz
RUN tar xzf scipoptsuite-6.0.1.tgz

WORKDIR /opt/scipoptsuite-6.0.1
RUN make

WORKDIR /opt/scipoptsuite-6.0.1/scip/interfaces/ampl/
RUN ./get.ASL

WORKDIR /opt/scipoptsuite-6.0.1/scip/interfaces/ampl/solvers/
RUN sh configurehere && make

WORKDIR /opt/scipoptsuite-6.0.1/scip/interfaces/ampl/
RUN make scipampl && make
RUN cp bin/scipampl /usr/local/bin/

# Install Gurobi solver
WORKDIR /opt
RUN wget https://packages.gurobi.com/8.1/gurobi8.1.0_linux64.tar.gz
RUN tar xzf gurobi8.1.0_linux64.tar.gz

WORKDIR /opt/gurobi810/linux64/
RUN python setup.py install
ENV GUROBI_HOME /opt/gurobi810/linux64/
ENV PATH ${PATH}:${GUROBI_HOME}/bin
ENV LD_LIBRARY_PATH ${LD_LIBRARY_PATH}:${GUROBI_HOME}/lib