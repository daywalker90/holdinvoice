#!/bin/bash
set -x
# Get the directory of the script
script_dir=$(dirname -- "$(readlink -f -- "$0")")

cargo_toml_path="$script_dir/../Cargo.toml"

# Use grep and awk to extract the version
version=$(awk -F'=' '/^\[package\]/ { in_package = 1 } in_package && /version/ { gsub(/[" ]/, "", $2); print $2; exit }' "$cargo_toml_path")

get_architecture() {
    machine=$(uname -m)

    case $machine in
        x86_64)
            echo 'x86_64-linux-gnu'
            ;;
        armv7l)
            echo 'armv7-linux-gnueabihf'
            ;;
        aarch64)
            echo 'aarch64-linux-gnu'
            ;;
        *)
            echo "No self-compiled binary found and unsupported release-architecture: $machine" >&2
            exit 1
            ;;
    esac
}
architecture=$(get_architecture)

github_url="https://github.com/daywalker90/holdinvoice/releases/download/v$version/holdinvoice-v$version-$architecture.tar.gz"


# Download the file using curl
if ! curl -L "$github_url" -o "$script_dir/holdinvoice-v$version-$architecture.tar.gz"; then
    echo "Error downloading the file from $github_url" >&2
    exit 1
fi

# Extract the contents using tar
if ! tar -xzvf "$script_dir/holdinvoice-v$version-$architecture.tar.gz" -C "$script_dir"; then
    echo "Error extracting the contents of holdinvoice-v$version-$architecture.tar.gz" >&2
    exit 1
fi

export PATH="$HOME/.local/bin:$PATH"

# Function to check if a Python package is installed
check_package() {
    python_exec="$1"
    package_name="$2"
    if $python_exec -c "import $package_name" &> /dev/null; then
        return 0
    else
        return 1
    fi
}

# Check if the package is installed in the first Python executable
if check_package "$TEST_DIR/bin/python3" "grpcio"; then
    python_exec="$TEST_DIR/bin/python3"
elif check_package "python3" "grpcio"; then
    python_exec="python3"
else
    echo "Error: Package 'grpcio' is not installed" >&2
    exit 1
fi

# Generate grpc files
if ! "$python_exec" -m grpc_tools.protoc --proto_path="$script_dir/../proto" --python_out=$script_dir --grpc_python_out=$script_dir hold.proto primitives.proto; then
    echo "Error generating grpc files" >&2
    exit 1
fi
