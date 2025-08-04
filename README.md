<table border="0">
  <tr>
    <td>
      <a href="https://github.com/daywalker90/holdinvoice/actions/workflows/latest_v24.08.yml">
        <img src="https://github.com/daywalker90/holdinvoice/actions/workflows/latest_v24.08.yml/badge.svg?branch=main">
      </a>
    </td>
    <td>
      <a href="https://github.com/daywalker90/holdinvoice/actions/workflows/main_v24.08.yml">
        <img src="https://github.com/daywalker90/holdinvoice/actions/workflows/main_v24.08.yml/badge.svg?branch=main">
      </a>
    </td>
  </tr>
  <tr>
    <td>
      <a href="https://github.com/daywalker90/holdinvoice/actions/workflows/latest_v24.11.yml">
        <img src="https://github.com/daywalker90/holdinvoice/actions/workflows/latest_v24.11.yml/badge.svg?branch=main">
      </a>
    </td>
    <td>
      <a href="https://github.com/daywalker90/holdinvoice/actions/workflows/main_v24.11.yml">
        <img src="https://github.com/daywalker90/holdinvoice/actions/workflows/main_v24.11.yml/badge.svg?branch=main">
      </a>
    </td>
  </tr>
  <tr>
    <td>
      <a href="https://github.com/daywalker90/holdinvoice/actions/workflows/latest_v25.02.yml">
        <img src="https://github.com/daywalker90/holdinvoice/actions/workflows/latest_v25.02.yml/badge.svg?branch=main">
      </a>
    </td>
    <td>
      <a href="https://github.com/daywalker90/holdinvoice/actions/workflows/main_v25.02.yml">
        <img src="https://github.com/daywalker90/holdinvoice/actions/workflows/main_v25.02.yml/badge.svg?branch=main">
      </a>
    </td>
  </tr>
  <tr>
    <td>
      <a href="https://github.com/daywalker90/holdinvoice/actions/workflows/latest_v25.05.yml">
        <img src="https://github.com/daywalker90/holdinvoice/actions/workflows/latest_v25.05.yml/badge.svg?branch=main">
      </a>
    </td>
    <td>
      <a href="https://github.com/daywalker90/holdinvoice/actions/workflows/main_v25.05.yml">
        <img src="https://github.com/daywalker90/holdinvoice/actions/workflows/main_v25.05.yml/badge.svg?branch=main">
      </a>
    </td>
  </tr>
</table>

# holdinvoice
Core lightning (CLN) plugin to hold invoices. Supports rpc and grpc.

* [Installation](#installation)
* [Building](#building)
* [Documentation](#documentation)
* [Options](#options)

# Installation
Release binaries for
* x86_64-linux
* armv7-linux (Raspberry Pi 32bit)
* aarch64-linux (Raspberry Pi 64bit)

can be found on the [release](https://github.com/daywalker90/holdinvoice/releases) page. If you are unsure about your architecture you can run ``uname -m``.

They require ``glibc>=2.31``, which you can check with ``ldd --version``.

In your CLN config you must add:

```
important-plugin=<path/to/holdinvoice>
```

and if you want to use the plugin via grpc you must add:

```
grpc-hold-port=<port>
```

to run a separate grpc server for the plugins methods.

# Building
You can build the plugin yourself instead of using the release binaries.
First clone the repo:

```
git clone https://github.com/daywalker90/holdinvoice.git
```

Install a recent rust version ([rustup](https://rustup.rs/) is recommended).

Install ``protobuf-compiler`` since we need ``protoc``:

```
apt install protobuf-compiler
```

Then in the ``holdinvoice`` folder run:

```
cargo build --release
```

After that the binary will be here: ``target/release/holdinvoice``

Note: Release binaries are built using ``cross`` and the ``optimized`` profile.

# Documentation
## Methods
These are the rpc/grpc methods provided by this plugin, for details check the ``hold.proto`` file in ``proto``:
* **holdinvoice**: *amount_msat* *description* [*expiry*] [*payment_hash*] [*preimage*] [*cltv*] [*deschashonly*] [*exposeprivatechannels*]
    * Create an invoice where the HTLC's will be held by the plugin.
    * It has almost the same options as CLN's ``invoice`` command. If `cltv` is not specified the default is `144` and not ``cltv-final`.
    * You can provide no `payment_hash`/`preimage` (a pair will be generated for you), both (a check if they match will be performed) or one of them, but if you just provide the `payment_hash` you must provide the `preimage` later when you want to settle the holdinvoice.
* **holdinvoicesettle**: *payment_hash* or *preimage*
    * Must be used with ``-k`` on the CLI to specify which argument you are passing.
    * Order the plugin to settle a holdinvoice with enough HTLC's being held. If you created the holdinvoice with just a `payment_hash` you must now provide the `preimage` here, otherwise you can use either.
    * Waits up to 30 seconds for HTLC's to actually settle, returns an error on timeout
* **holdinvoicecancel**: *payment_hash*
    * Order the plugin to cancel a holdinvoice and return any pending HTLC's back.
    * Waits up to 30 seconds for HTLC's to actually cancel, returns an error on timeout
* **holdinvoicelookup**: [*payment_hash*]
    * Look up all holdinvoices or just the one with the provided `payment_hash`
    * ``state`` field can be:
        * ``OPEN`` (no or not enough HTLC's pending)
        * ``ACCEPTED`` (enough HTLC's to fulfill the invoice pending)
        * ``SETTLED`` (invoice paid)
        * ``CANCELED`` (invoice unpaid and will not accept any further HTLC's even if not yet expired)
* **holdinvoice-version**:
    * Returns an object containing `version` with the version of the plugin

## Notification
* **holdinvoice_accepted**: a notification that is send out for rpc and grpc when a holdinvoice switches into the `ACCEPTED` state.
    * Notification contains the `payment_hash` and the `htlc_expiry` of the HTLC that will expire first

The plugin will automatically settle any holdinvoice if a pending HTLC is close to expiry and would otherwise cause a force close of the channel. This is of course only possible if the plugin already knows the preimage, otherwise it will cancel at this point. You can configure when the block expiry happens with the option below. If for some reason the plugin was not able to settle a holdinvoice in time (e.g. your node was down) the plugin will CANCEL the holdinvoice! 

# Options
You can set the following option(s) in your CLN config file:

* ``grpc-hold-port``: Set this to your desired port for the grpc server, unset it defaults to disabling grpc
* ``holdinvoice-cancel-before-htlc-expiry``: number of blocks before HTLC's expiry where the plugin auto-cancels invoice and HTLC's, Default: ``6``
* ``holdinvoice-startup-lock``: time in seconds after the start of the plugin to wait before answering to rpc commands. This is needed when there are HTLC's being replayed to the plugin during a node restart so the rpc commands will return consistent states for holdinvoices. This is also the grace period for HTLC's for expired invoices to still be accepted during a node restart. Defaults to ``20``s

