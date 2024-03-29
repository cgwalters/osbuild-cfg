# osbuild-cfg

A small tool that accepts an opinionated subset of operating system configuration
tasks designed to pair with [bootc](https://github.com/containers/bootc) systems.

This program is designed to execute as part of a container image build,
supporting existing declarative input formats such as
[RHEL Image Builder blueprints](https://access.redhat.com/documentation/en-us/red_hat_enterprise_linux/9/html/composing_a_customized_rhel_system_image/creating-system-images-with-composer-command-line-interface_composing-a-customized-rhel-system-image#composer-blueprint-format_creating-system-images-with-composer-command-line-interface).

In the future, more formats may be supported such as Kickstart, Butane/Ignition, etc.

## Using in a container build

```
# Fetch this tool as a container image
FROM ghcr.io/cgwalters/osbuild-cfg as osbuildcfg
# Derive from your chosen bootable container image
FROM <bootc base image>
# Inject your blueprint
COPY myblueprint.toml /tmp/myblueprint.toml
# Copy in the binary
COPY --from=osbuildcfg /usr/bin/osbuildcfg /osbuildcfg
# Run this tool, which will consume itself *and* the input
RUN /osbuildcfg /tmp/myblueprint.toml
```

## Blueprints

Only a subset of the blueprint format is supported.   At the current time:

- `[[customizations.sshkey]]`, and only for the `root` user
- `[[packages]]`

Note that the implementation of root SSH keys uses a systemd `tmpfiles.d` snippet
injected into `/usr/lib/tmpfiles.d`, it does not directly write to the `/root` home
directory.  The rationale for this is to better handle "less stateful" systems where
`/root` may be mounted as a `tmpfs`.

The implementation of `packages` is a thin wrapper for executing `dnf install`.

### Blueprint examples

```
[[customizations.sshkey]]
user = "root"
key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA1hVwAoNDa64+woHYxkHfdu6qmdC1BsLXReeQz/CBri example@demo"
```

```
[[packages]]
name = "httpd"
version = "2.4"
[[packages]]
name = "mariadb-server"
[[packages]]
name = "mariadb"  
[[packages]]
name = "php"
version = "5.1"
```

