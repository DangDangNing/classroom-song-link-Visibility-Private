# Song Link

一个局域网点歌小站。学生用手机打开点歌链接提交歌曲，老师在管理页查看队列并自动播放音乐预览。

## 运行

```bash
cargo run
```

学生点歌页：<http://localhost:7878/>

老师管理页：<http://localhost:7878/admin>

## 局域网使用

运行程序的电脑和手机连接同一个 Wi-Fi 后，手机访问：

```text
http://电脑局域网IP:7878/
```

老师访问：

```text
http://电脑局域网IP:7878/admin
```

如果手机打不开，通常需要允许防火墙放行 `7878` 端口，或检查当前 Wi-Fi 是否开启了设备隔离。

## 文件结构

- `src/main.rs`：Rust 后端与 JSON API
- `web/public.html`：学生点歌页面
- `web/admin.html`：老师管理页面

## 可选项

换端口：

```powershell
$env:PORT="9000"; cargo run
```
