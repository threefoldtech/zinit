# Build an example docker image that uses
# zinit as PID1 and starts redis and sshd

# This docker image is for demonstration purpose only
# and can be used as a seed for more complicated images
# was more services

FROM ubuntu:bionic
COPY zinit /sbin/
RUN apt-get update
RUN apt-get install -y redis openssh-server
RUN apt-get clean

EXPOSE 22 6379

RUN mkdir /etc/zinit
RUN echo "exec: redis-server\ntest: redis-cli ping" > /etc/zinit/redis.yaml

RUN echo "exec: mkdir -p /run/sshd" > /etc/zinit/sshd-setup.yaml
RUN echo "exec: /usr/sbin/sshd -D\nafter:\n  - sshd-setup" > /etc/zinit/sshd.yaml

ENTRYPOINT /sbin/zinit init