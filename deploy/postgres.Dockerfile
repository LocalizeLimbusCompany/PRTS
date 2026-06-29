# PRTS Postgres：pgvector 基础上叠加 SCWS + zhparser（中文全文分词）。
FROM pgvector/pgvector:pg16

ARG SCWS_VER=1.2.3
RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
        build-essential wget ca-certificates postgresql-server-dev-16 git; \
    # SCWS
    wget -O /tmp/scws.tar.bz2 "http://www.xunsearch.com/scws/down/scws-${SCWS_VER}.tar.bz2"; \
    mkdir -p /tmp/scws && tar -xjf /tmp/scws.tar.bz2 -C /tmp/scws --strip-components=1; \
    cd /tmp/scws && ./configure && make && make install; \
    # zhparser
    git clone --depth 1 https://github.com/amutu/zhparser.git /tmp/zhparser; \
    cd /tmp/zhparser && SCWS_HOME=/usr/local make && make install; \
    apt-get purge -y --auto-remove build-essential wget git postgresql-server-dev-16; \
    rm -rf /var/lib/apt/lists/* /tmp/scws* /tmp/zhparser; \
    ldconfig
