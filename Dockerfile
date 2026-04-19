FROM golang:1.25.3-alpine AS builder

WORKDIR /app

ARG GOPROXY=https://goproxy.cn,direct
ARG GOSUMDB=off
ARG HTTP_PROXY
ARG HTTPS_PROXY
ARG NO_PROXY

ENV GOPROXY=$GOPROXY
ENV GOSUMDB=$GOSUMDB
ENV HTTP_PROXY=$HTTP_PROXY
ENV HTTPS_PROXY=$HTTPS_PROXY
ENV NO_PROXY=$NO_PROXY

COPY go.mod go.sum ./
RUN go mod download

COPY . .

RUN CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build -o /bin/prts-api ./cmd/api
RUN CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build -o /bin/prts-worker ./cmd/worker
RUN CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build -o /bin/prts-migrate ./cmd/migrate

FROM alpine:3.22

WORKDIR /app

RUN addgroup -S app && adduser -S app -G app

COPY --from=builder /bin/prts-api /usr/local/bin/prts-api
COPY --from=builder /bin/prts-worker /usr/local/bin/prts-worker
COPY --from=builder /bin/prts-migrate /usr/local/bin/prts-migrate
COPY db/migrations /app/db/migrations

USER app

EXPOSE 18080

CMD ["prts-api"]
