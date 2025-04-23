FROM ubuntu:18.04

RUN mkdir -p /etc/zinit
ADD zinit /sbin/zinit

ENTRYPOINT ["/sbin/zinit", "init"] 