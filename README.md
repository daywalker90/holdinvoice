[![latest release on CLN v24.08.1](https://github.com/daywalker90/holdinvoice/actions/workflows/latest_v24.08.yml/badge.svg?branch=main)](https://github.com/daywalker90/holdinvoice/actions/workflows/latest_v24.08.yml) [![latest release on CLN v24.05](https://github.com/daywalker90/holdinvoice/actions/workflows/latest_v24.05.yml/badge.svg?branch=main)](https://github.com/daywalker90/holdinvoice/actions/workflows/latest_v24.05.yml) [![latest release on CLN v24.02.2](https://github.com/daywalker90/holdinvoice/actions/workflows/latest_v24.02.yml/badge.svg?branch=main)](https://github.com/daywalker90/holdinvoice/actions/workflows/latest_v24.02.yml) 

[![main on CLN v24.08.1](https://github.com/daywalker90/holdinvoice/actions/workflows/main_v24.08.yml/badge.svg?branch=main)](https://github.com/daywalker90/holdinvoice/actions/workflows/main_v24.08.yml) [![main on CLN v24.05](https://github.com/daywalker90/holdinvoice/actions/workflows/main_v24.05.yml/badge.svg?branch=main)](https://github.com/daywalker90/holdinvoice/actions/workflows/main_v24.05.yml) [![main on CLN v24.02.2](https://github.com/daywalker90/holdinvoice/actions/workflows/main_v24.02.yml/badge.svg?branch=main)](https://github.com/daywalker90/holdinvoice/actions/workflows/main_v24.02.yml) 

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

In your cln config you must add:

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
There are four methods provided by this plugin:
* ``holdinvoice``: amount_msat label description [expiry]
[fallbacks] [preimage] cltv [deschashonly] 
    * create an invoice where the HTLC's will be held by the plugin, it has almost the same options as cln's invoice, but cltv is required
* ``holdinvoicesettle``: payment_hash 
    * order plugin to settle a holdinvoice with enough HTLC's being held, does not wait for actual setllement of HTLC's
* ``holdinvoicecancel``: payment_hash
    * order plugin to cancel a holdinvoice and return any pending HTLC's back, does not wait for actual return of HTLC's
* ``holdinvoicelookup``: payment_hash
    * look up the holdstate of a holdinvoice and if it's in the ACCEPTED holdstate return the ``htlc_expiry``
    * waits for actual settlement or return of HTLC's (with a timeout) and doublechecks holdstate with invoice state
    * valid holdstates are:
        * OPEN (no or not enough HTLC's pending)
        * ACCEPTED (enough HTLC's to fulfill the invoice pending)
        * SETTLED (invoice paid)
        * CANCELED (invoice unpaid and will not accept any further HTLC's even if not yet expired)

The plugin will automatically cancel any invoice if *it* is either close to expiry (this is one major difference to the way lnd does it because cln can't settle with an expired invoice) or if a pending HTLC is close to expiry and would otherwise cause a force close of the channel. You can configure when this happens with the options below.

During a node restart invoices that were previously in the ACCEPTED state can temporarily be back in the OPEN state, because the HTLC's get replayed to the plugin during startup.

# Options
You can set the following options in your cln config file:

* ``holdinvoice-cancel-before-htlc-expiry``: number of blocks before HTLC's expiry where the plugin auto-cancels invoice and HTLC's, Default: ``6``
* ``holdinvoice-cancel-before-invoice-expiry``: number of seconds before invoice expiry where the plugin auto cancels any pending HTLC's and no longer accepts new HTLC's, Default: ``1800``