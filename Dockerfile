FROM golang:1.25.3-alpine AS builder

WORKDIR /app

COPY go.mod ./
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
