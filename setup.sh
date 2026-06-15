#!/usr/bin/env bash
#
# setup.sh — 将 target/release/myQQBot 部署为 systemd 服务
#
# 用法:
#   sudo bash setup.sh              # 交互式部署
#   sudo bash setup.sh --help       # 显示帮助
#
# 环境变量 (可覆盖默认值):
#   BOT_PATH     二进制文件路径，默认 target/release/myQQBot
#   CONFIG_PATH  配置文件路径，默认 config.toml
#   INSTALL_DIR  安装目录，默认 /opt/myQQBot
#   USER         运行服务的系统用户，默认 myqqbot
#   GROUP        运行服务的系统组， 默认 myqqbot
#

set -euo pipefail

# ──────────────────────────────────────────────
# 默认值
# ──────────────────────────────────────────────
PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

BOT_PATH="${BOT_PATH:-$PROJECT_DIR/target/release/myQQBot}"
CONFIG_PATH="${CONFIG_PATH:-$PROJECT_DIR/config.toml}"
INSTALL_DIR="${INSTALL_DIR:-/opt/myQQBot}"
SERVICE_USER="${USER:-myqqbot}"
SERVICE_GROUP="${GROUP:-myqqbot}"

SERVICE_NAME="myqqbot"
SERVICE_FILE="/etc/systemd/system/${SERVICE_NAME}.service"

# ──────────────────────────────────────────────
# 辅助函数
# ──────────────────────────────────────────────
info()  { echo -e "\e[32m[INFO]\e[0m  $*"; }
warn()  { echo -e "\e[33m[WARN]\e[0m  $*"; }
error() { echo -e "\e[31m[ERROR]\e[0m $*" >&2; }

usage() {
    cat <<EOF
用法: sudo bash $0 [选项]

将 target/release/myQQBot 部署为 systemd 服务。

选项:
  --help         显示此帮助信息

环境变量:
  BOT_PATH      二进制文件路径 (默认: $PROJECT_DIR/target/release/myQQBot)
  CONFIG_PATH   配置文件路径 (默认: $PROJECT_DIR/config.toml)
  INSTALL_DIR   安装目录 (默认: /opt/myQQBot)
  USER          运行服务的系统用户 (默认: myqqbot)
  GROUP         运行服务的系统组 (默认: myqqbot)
EOF
    exit 0
}

# ──────────────────────────────────────────────
# 前置检查
# ──────────────────────────────────────────────
check_prerequisites() {
    if [[ $EUID -ne 0 ]]; then
        error "此脚本需要 root 权限，请使用 sudo 执行。"
        exit 1
    fi

    if [[ ! -f "$BOT_PATH" ]]; then
        error "找不到二进制文件: $BOT_PATH"
        echo "  请先执行 cargo build --release 编译项目。"
        exit 1
    fi

    if [[ ! -x "$BOT_PATH" ]]; then
        error "二进制文件没有执行权限: $BOT_PATH"
        exit 1
    fi

    if [[ ! -f "$CONFIG_PATH" ]]; then
        warn "找不到配置文件: $CONFIG_PATH"
        echo "  将使用默认配置部署，请稍后手动创建 $INSTALL_DIR/config.toml。"
    fi

    if ! command -v systemctl &>/dev/null; then
        error "系统不支持 systemd（未找到 systemctl 命令）。"
        exit 1
    fi
}

# ──────────────────────────────────────────────
# 创建系统用户/组
# ──────────────────────────────────────────────
create_user() {
    if getent group "$SERVICE_GROUP" &>/dev/null; then
        info "系统组 $SERVICE_GROUP 已存在，跳过创建。"
    else
        info "创建系统组: $SERVICE_GROUP"
        groupadd --system "$SERVICE_GROUP"
    fi

    if getent passwd "$SERVICE_USER" &>/dev/null; then
        info "系统用户 $SERVICE_USER 已存在，跳过创建。"
    else
        info "创建系统用户: $SERVICE_USER"
        useradd --system \
                --gid "$SERVICE_GROUP" \
                --no-create-home \
                --shell /usr/sbin/nologin \
                "$SERVICE_USER"
    fi
}

# ──────────────────────────────────────────────
# 部署文件
# ──────────────────────────────────────────────
deploy_files() {
    info "创建安装目录: $INSTALL_DIR"
    mkdir -p "$INSTALL_DIR"

    info "复制二进制文件: $BOT_PATH → $INSTALL_DIR/myQQBot"
    cp "$BOT_PATH" "$INSTALL_DIR/myQQBot"
    chmod 755 "$INSTALL_DIR/myQQBot"

    if [[ -f "$CONFIG_PATH" ]]; then
        info "复制配置文件: $CONFIG_PATH → $INSTALL_DIR/config.toml"
        cp "$CONFIG_PATH" "$INSTALL_DIR/config.toml"
        chmod 640 "$INSTALL_DIR/config.toml"
    fi

    info "设置目录权限: $INSTALL_DIR"
    chown -R "$SERVICE_USER:$SERVICE_GROUP" "$INSTALL_DIR"
}

# ──────────────────────────────────────────────
# 生成 systemd service 文件
# ──────────────────────────────────────────────
generate_service() {
    info "生成 systemd service 文件: $SERVICE_FILE"

    cat > "$SERVICE_FILE" <<SERVICEEOF
[Unit]
Description=myQQBot - QQ 官方机器人服务
Documentation=https://github.com/your-username/myQQBot
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=$SERVICE_USER
Group=$SERVICE_GROUP

WorkingDirectory=$INSTALL_DIR
ExecStart=$INSTALL_DIR/myQQBot
Restart=on-failure
RestartSec=5

# 安全加固
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=full
ProtectHome=true

[Install]
WantedBy=multi-user.target
SERVICEEOF

    chmod 644 "$SERVICE_FILE"
}

# ──────────────────────────────────────────────
# 启动服务
# ──────────────────────────────────────────────
start_service() {
    info "重新加载 systemd 守护进程..."
    systemctl daemon-reload

    info "启用开机自启: $SERVICE_NAME"
    systemctl enable "$SERVICE_NAME"

    info "启动服务: $SERVICE_NAME"
    systemctl start "$SERVICE_NAME"

    echo ""
    info "服务状态:"
    systemctl status "$SERVICE_NAME" --no-pager || true
}

# ──────────────────────────────────────────────
# 主流程
# ──────────────────────────────────────────────
main() {
    if [[ "${1:-}" == "--help" ]]; then
        usage
    fi

    echo "=============================================="
    echo "  myQQBot systemd 部署脚本"
    echo "=============================================="
    echo ""
    echo "  二进制文件:  $BOT_PATH"
    echo "  配置文件:    $CONFIG_PATH"
    echo "  安装目录:    $INSTALL_DIR"
    echo "  运行用户:    $SERVICE_USER"
    echo "  服务名称:    $SERVICE_NAME"
    echo ""

    check_prerequisites
    create_user
    deploy_files
    generate_service
    start_service

    echo ""
    echo "=============================================="
    info "部署完成！"
    echo ""
    echo "  管理命令:"
    echo "    sudo systemctl status $SERVICE_NAME    # 查看状态"
    echo "    sudo systemctl restart $SERVICE_NAME   # 重启服务"
    echo "    sudo systemctl stop $SERVICE_NAME      # 停止服务"
    echo "    sudo journalctl -u $SERVICE_NAME -f    # 查看实时日志"
    echo ""
    echo "  配置文件: $INSTALL_DIR/config.toml"
    echo "=============================================="
}

main "$@"
