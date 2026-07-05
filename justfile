name := 'cosmic-ext-applet-workspace-icons'
appid := 'io.github.crocodile.cosmic-ext-applet-workspace-icons'
rootdir := ''
prefix := env('PREFIX', home_directory() / '.local')
targetdir := env('CARGO_TARGET_DIR', 'target')
bindir := clean(rootdir / prefix) / 'bin'
sharedir := clean(rootdir / prefix) / 'share'

default: build

build *args:
    cargo build --release {{ args }}

install: build
    install -Dm0755 {{ targetdir }}/release/{{ name }} {{ bindir }}/{{ name }}
    install -Dm0644 resources/{{ appid }}.desktop {{ sharedir }}/applications/{{ appid }}.desktop
    install -Dm0644 resources/{{ appid }}.metainfo.xml {{ sharedir }}/metainfo/{{ appid }}.metainfo.xml
    install -Dm0644 resources/{{ appid }}.svg {{ sharedir }}/icons/hicolor/scalable/apps/{{ appid }}.svg

uninstall:
    rm -f {{ bindir }}/{{ name }}
    rm -f {{ sharedir }}/applications/{{ appid }}.desktop
    rm -f {{ sharedir }}/metainfo/{{ appid }}.metainfo.xml
    rm -f {{ sharedir }}/icons/hicolor/scalable/apps/{{ appid }}.svg
