FROM registry.fedoraproject.org/fedora:43

# Build dependencies for the GTK4/libadwaita/WebKitGTK app, plus runtime bits
# needed to open a window from inside the container.
RUN dnf install -y --setopt=install_weak_deps=False \
      gcc gcc-c++ make pkgconf-pkg-config git sudo curl which procps-ng \
      gtk4-devel libadwaita-devel webkitgtk6.0-devel gtksourceview5-devel \
      adwaita-icon-theme adwaita-fonts-all google-noto-sans-fonts \
      mesa-dri-drivers mesa-libEGL mesa-libGL dbus-daemon dbus-x11 \
      xdg-utils shared-mime-info \
    && dnf clean all

RUN rm -f /etc/machine-id && dbus-uuidgen --ensure=/etc/machine-id

# Non-root user (matches the host UID so bind-mounted files stay editable)
ARG USERNAME=vscode
ARG UID=1000
RUN useradd -m -u ${UID} -s /bin/bash ${USERNAME} \
    && echo "${USERNAME} ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/${USERNAME}

USER ${USERNAME}
ENV CARGO_HOME=/home/${USERNAME}/.cargo RUSTUP_HOME=/home/${USERNAME}/.rustup \
    PATH=/home/${USERNAME}/.cargo/bin:$PATH
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile default --default-toolchain stable


RUN curl -fsSL https://claude.ai/install.sh | bash
ENV PATH=/home/${USERNAME}/.local/bin:$PATH