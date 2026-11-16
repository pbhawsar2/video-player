FROM registry.fedoraproject.org/fedora:45

RUN dnf -y install --setopt=install_weak_deps=False \
    rust cargo gcc pkgconf-pkg-config \
    gtk4-devel libadwaita-devel \
    gstreamer1-devel gstreamer1-plugins-base-devel \
    openssl-devel \
    && dnf clean all

WORKDIR /workspace
