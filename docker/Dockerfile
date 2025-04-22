FROM ubuntu:18.04

RUN mkdir -p /etc/zinit
ADD zinit /sbin/zinit
# ADD zinit-http /sbin/zinit-http

ENTRYPOINT ["/sbin/zinit", "init"] 