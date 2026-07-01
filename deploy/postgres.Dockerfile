# PRTS Postgres：在 pgvector 基础上叠加 SCWS + zhparser（中文全文分词）。
#
# SCWS 从 GitHub(HTTPS) 源码构建，替代原先的 xunsearch.com(HTTP) 压缩包下载——
# 后者仅有 HTTP、境内常不可达，且 wget 无超时会让 `docker compose up --build` 无限挂起。
# GitHub 源码不含预生成的 configure，故需 autotools 引导。
FROM pgvector/pgvector:pg16

RUN set -eux; \
    # 网络操作快速失败：低速（<1KB/s 持续 60s）即中止，避免克隆停滞导致无限挂起。
    git config --global http.lowSpeedLimit 1000; \
    git config --global http.lowSpeedTime 60; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
        build-essential ca-certificates git \
        autoconf automake libtool m4 libxml2-dev \
        postgresql-server-dev-16; \
    # 确保运行期 libxml2 保留（SCWS 运行期依赖；postgres 亦依赖，双保险，防被 auto-remove）。
    apt-get install -y --no-install-recommends libxml2; \
    # —— SCWS 1.2.3（GitHub 源码；无预生成 configure，需 autotools 引导）——
    git clone --branch 1.2.3 --single-branch --depth 1 https://github.com/hightman/scws.git /tmp/scws; \
    cd /tmp/scws; \
    touch README; \
    aclocal; \
    autoconf; \
    autoheader; \
    libtoolize --force; \
    automake --add-missing; \
    ./configure --prefix=/usr/local; \
    make -j"$(nproc)"; \
    make install; \
    # —— zhparser（针对 SCWS_HOME=/usr/local 构建）——
    git clone --depth 1 https://github.com/amutu/zhparser.git /tmp/zhparser; \
    cd /tmp/zhparser; \
    SCWS_HOME=/usr/local make; \
    make install; \
    # —— 清理构建依赖，缩小镜像（保留 libxml2 运行库）——
    apt-get purge -y --auto-remove \
        build-essential git autoconf automake libtool m4 libxml2-dev postgresql-server-dev-16; \
    rm -rf /var/lib/apt/lists/* /tmp/scws /tmp/zhparser; \
    ldconfig
