// The way through the front door. The control protocol is an http upgrade
// that is not websocket, and a proxy like Cloudflare's strips it; the
// server accepts the same bytes over a real websocket (that is how the
// browser build of Tailscale talks to it). This listens on localhost,
// takes the engine's plain upgrade, opens a websocket to the control
// server, and pipes. Everything else the engine asks of its control url
// (the server's key, for one) is forwarded as the plain request it is.
//
// The app's engine runs one in-process; a box runs one as a service
// (cmd/netbridge) in front of its stock tailscaled, which otherwise dials
// the control server's address directly and, on a box already on another
// tailnet, has no route to it.
package bridge

import (
	"context"
	"encoding/base64"
	"io"
	"log"
	"net"
	"net/http"
	"net/http/httputil"
	"net/url"
	"strings"

	"github.com/coder/websocket"
)

const (
	upgradePath    = "/ts2021"
	upgradeValue   = "tailscale-control-protocol"
	handshakeParam = "X-Tailscale-Handshake"
)

// Bridge serves the engine's control traffic from a loopback address and
// carries it to the real control server.
type Bridge struct {
	control *url.URL
	ln      net.Listener
}

// New listens on listen (a loopback address; port 0 for any) and carries
// what arrives to the control server at control.
func New(control, listen string) (*Bridge, error) {
	u, err := url.Parse(control)
	if err != nil {
		return nil, err
	}
	ln, err := net.Listen("tcp", listen)
	if err != nil {
		return nil, err
	}
	b := &Bridge{control: u, ln: ln}
	proxy := httputil.NewSingleHostReverseProxy(u)
	director := proxy.Director
	proxy.Director = func(r *http.Request) {
		director(r)
		r.Host = u.Host
	}
	mux := http.NewServeMux()
	mux.HandleFunc(upgradePath, func(w http.ResponseWriter, r *http.Request) {
		if strings.EqualFold(r.Header.Get("Upgrade"), upgradeValue) {
			b.carry(w, r)
			return
		}
		proxy.ServeHTTP(w, r)
	})
	mux.Handle("/", proxy)
	go func() {
		srv := &http.Server{Handler: mux, ErrorLog: log.New(io.Discard, "", 0)}
		_ = srv.Serve(ln)
	}()
	return b, nil
}

// URL is what the engine is told its control server is
func (b *Bridge) URL() string {
	return "http://" + b.ln.Addr().String()
}

func (b *Bridge) Close() {
	b.ln.Close()
}

// carry: one control connection, the engine's upgrade on one side and a
// websocket to the server on the other, the handshake moved from the
// header into the query as the websocket form has it
func (b *Bridge) carry(w http.ResponseWriter, r *http.Request) {
	init := r.Header.Get(handshakeParam)
	if _, err := base64.StdEncoding.DecodeString(init); err != nil || init == "" {
		http.Error(w, "missing handshake", http.StatusBadRequest)
		return
	}
	hj, ok := w.(http.Hijacker)
	if !ok {
		http.Error(w, "cannot take over the connection", http.StatusInternalServerError)
		return
	}
	scheme := "wss"
	if b.control.Scheme == "http" {
		scheme = "ws"
	}
	wsURL := &url.URL{
		Scheme:   scheme,
		Host:     b.control.Host,
		Path:     upgradePath,
		RawQuery: url.Values{handshakeParam: []string{init}}.Encode(),
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	ws, _, err := websocket.Dial(ctx, wsURL.String(), &websocket.DialOptions{
		Subprotocols:    []string{upgradeValue},
		CompressionMode: websocket.CompressionDisabled,
	})
	if err != nil {
		http.Error(w, "control server: "+err.Error(), http.StatusBadGateway)
		return
	}
	server := websocket.NetConn(ctx, ws, websocket.MessageBinary)
	client, rw, err := hj.Hijack()
	if err != nil {
		server.Close()
		return
	}
	defer client.Close()
	defer server.Close()
	_, _ = rw.WriteString("HTTP/1.1 101 Switching Protocols\r\nUpgrade: " + upgradeValue + "\r\nConnection: upgrade\r\n\r\n")
	if err := rw.Flush(); err != nil {
		return
	}
	done := make(chan struct{}, 2)
	// what the client sent past its headers is in the buffered reader
	go func() { _, _ = io.Copy(server, rw.Reader); done <- struct{}{} }()
	go func() { _, _ = io.Copy(client, server); done <- struct{}{} }()
	<-done
}
