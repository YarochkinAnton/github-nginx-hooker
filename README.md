# GitHub Nginx hooker

Automaticaly update nginx allow list with GitHub hook server ip addresses

## What?

This program fetches GitHub API for [meta](https://docs.github.com/en/rest/meta)
information. And writes allow statements in specified file

## Why?

I am really lazy and don't want my Jenkins server to be fully accessible from
the Internet

## Build requirements

- git
- [rust](https://rustup.rs/)

## Example usage

You have Jenkins server running behind Nginx reverse-proxy.

By default access to all of your sites denied by `deny all;` statement.

You want to allow requests to `/github-webhook/` Jenkins route only from GitHub hook
server

---

### Clone and build this shit

```bash
cd /opt
git clone https://github.com/YarochkinAnton/github-nginx-hooker.git
cd github-nginx-hooker
cargo build --release
```

---

### Update Nginx configuration

Locate your Jenkins site config (e.g. `/etc/nginx/sites-available/jenkins.conf`)
and add following configuration

```nginx
server {
    server_name jenkins.example.com;
    ...
    location /github-webhook/ {
        include /etc/nginx/snippets/github_webhook.conf;

        proxy_pass http://{{ JENKINS_IP_AND_PORT }};
    }
    ...
}
```

---

### Config this shit

Then create config file for the program (e.g. `/etc/hooker.toml`)

```toml
# https://github.com/settings/tokens
token = "YEAH RIGHT"

# Path to file that will contain allow statements
allow_file = "/etc/nginx/snippets/github_webhook.conf"

# Time interval between checks (in seconds)
repeat = 30

# Command to execute after hook server ip list change
after_update_hook = "nginx -s reload"
```

---

### Daemonize this shit

Then you can create systemd service for this program.

Create a systemd unit file (e.g. `/etc/systemd/system/github-nginx-hooker.service`)

```ini
[Unit]
Description = "GitHub hook server ip list updater for Nginx"

[Service]
WorkingDirectory=/opt/github-nginx-hooker
ExecStart=/opt/github-nginx-hooker/target/release/github-nginx-hooker /etc/hooker.toml
Environment="RUST_LOG=info"

[Install]
WantedBy=multi-user.target
```

Reload systemd unit files

```bash
systemctl daemon-reload
```

Enable and start the service

```bash
systemctl enable github-nginx-hooker
systemctl start github-nginx-hooker
```

Use `journalctl` to see logs

```bash
journalctl -fu github-nginx-hooker
```

```log
github-nginx-hooker[1975790]: [2222-09-08T00:00:18Z INFO  github_nginx_hooker] Update cycle completed
github-nginx-hooker[1975790]: [2222-09-08T00:00:18Z INFO  github_nginx_hooker] Allow list is CHANGED
github-nginx-hooker[1975790]: [2222-09-08T00:00:49Z INFO  github_nginx_hooker] Update cycle completed
github-nginx-hooker[1975790]: [2222-09-08T00:00:49Z INFO  github_nginx_hooker] Allow list is UNCHANGED
```
