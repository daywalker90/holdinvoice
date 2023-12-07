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
There are five methods provided by this plugin:
* (GRPC ONLY) ``DecodeBolt11``:  bolt11
    * To get routehints, which the rust crates currently do not provide
* ``holdinvoice``: amount_msat label description [expiry]
[fallbacks] [preimage] cltv [deschashonly] 
    * create an invoice where the htlcs will be held by the plugin, it has almost the same options as cln's invoice, but cltv is required
* ``holdinvoicesettle``: payment_hash 
    * order plugin to settle a holdinvoice with enough htlcs being held, does not wait for actual setllement of htlcs
* ``holdinvoicecancel``: payment_hash
    * order plugin to cancel a holdinvoice and return any pending htlcs back, does not wait for actual return of htlcs
* ``holdinvoicelookup``: payment_hash
    * look up the holdstate of a holdinvoice and if it's in the ACCEPTED holdstate return the ``htlc_expiry``
    * waits for actual settlement or return of htlcs (with a timeout) and doublechecks holdstate with invoice state
    * valid holdstates are:
        * OPEN (no or not enough htlcs pending)
        * ACCEPTED (enough htlcs to fulfill the invoice pending)
        * SETTLED (invoice paid)
        * CANCELED (invoice unpaid and will not accept any further htlcs even if not yet expired)

The plugin will automatically cancel any invoice if *it* is either close to expiry (this is one major difference to the way lnd does it because cln can't settle with an expired invoice) or if a pending htlc is close to expiry and would otherwise cause a force close of the channel. You can configure when this happens with the options below.

# Options
You can set the following options in your cln config file:

* ``holdinvoice-cancel-before-htlc-expiry``: number of blocks before htlcs expiry where the plugin auto-cancels invoice and htlcs, Default: ``6``
* ``holdinvoice-cancel-before-invoice-expiry``: number of seconds before invoice expiry where the plugin auto cancels any pending htlcs and no longer accepts new htlcs, Default: ``1800``