> Work in progress 
this repository is heavly under development and is still not usable at the moment. We still trying to figure out the basic optimum structure for the process manager

# zinit
A POC PID 1 replacement that feels like runit written in rust+tokio

## Goal
A PID replacement that is very lightweight and provide the following requirements
- Make sure that configured services are up and running at all times
- Support service dependencies during the boot process
- Provide a simple command line interface to add, start, stop and reload services

