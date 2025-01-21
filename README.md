# Sockonsole
Sockonsole is a basic utility to be used for running something like a shell in the background, and be able to connect and interact with it whenever you want, through unix sockets.
## Installation
In a terminal, paste the following
```sh
cargo install --locked sockonsole
```
## Usage
`sockonsole start` this will start your command. This must be run first in order to connect to it.
`sockonsole stop` will stop your command
`sockonsole connect` will connect your terminal to the command running, allowing you to interact with it
## Configuration
Create your configuration file at `$HOME/.config/sockonsole/config.toml`
And paste the following default configuration in
```toml
command = "/bin/sh"
response_timeout = 100

[env_vars]
VAR1="test"
VAR2="hello world"
```

#### command
The command field should be a string, and it is the interactive program (like a shell) that sockonsole will run in the background
#### response_timeout
This should be a positive integer, and corresponds to the amount of milliseconds that sockonsole will wait for your command to give output before considering output "finished"
#### env_vars
These are key value pairs for environment variables that your command will inherit