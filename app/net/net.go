// The fleet's network engine for the app: Tailscale's tsnet behind a C
// interface of four calls. Nothing here knows what Commonty is; it takes
// a control server, a key and a state directory, joins, and hands back a
// loopback proxy that routes into the network and resolves its names.
// The Rust side does everything else through that proxy.
package main

/*
#include <stdlib.h>
*/
import "C"

import (
	"context"
	"encoding/json"
	"log"
	"os"
	"sync"
	"time"
	"unsafe"

	"tailscale.com/tsnet"
)

var (
	mu    sync.Mutex
	srv   *tsnet.Server
	door  *bridge
	proxy *proxyServer
)

type started struct {
	Proxy      string `json:"proxy"`
	Credential string `json:"credential"`
	IP         string `json:"ip"`
	Error      string `json:"error,omitempty"`
}

type peer struct {
	Name   string `json:"name"`
	IP     string `json:"ip"`
	Online bool   `json:"online"`
}

type status struct {
	Running bool   `json:"running"`
	State   string `json:"state"`
	IP      string `json:"ip"`
	Name    string `json:"name"`
	Peers   []peer `json:"peers"`
	Error   string `json:"error,omitempty"`
}

func reply(v any) *C.char {
	b, _ := json.Marshal(v)
	return C.CString(string(b))
}

// commonty_net_start joins the network (or resumes from the state in dir
// when key is empty) and returns json: the SOCKS5 proxy's address and
// password, this node's address. Idempotent while running.
//
//export commonty_net_start
func commonty_net_start(dir, control, key, hostname *C.char) *C.char {
	mu.Lock()
	defer mu.Unlock()
	if srv != nil {
		return reply(started{Proxy: proxy.addr(), Credential: proxy.cred, IP: ipOf(srv)})
	}
	// the control server is reached through the bridge: the front door
	// carries websockets, not the engine's own upgrade
	b, err := newBridge(C.GoString(control))
	if err != nil {
		return reply(started{Error: err.Error()})
	}
	// quiet unless asked: the engine's log is a firehose
	logf := func(string, ...any) {}
	if os.Getenv("COMMONTY_NET_DEBUG") != "" {
		logf = log.Printf
	}
	s := &tsnet.Server{
		Dir:        C.GoString(dir),
		ControlURL: b.url(),
		AuthKey:    C.GoString(key),
		Hostname:   C.GoString(hostname),
		Ephemeral:  false,
		Logf:       logf,
		UserLogf:   logf,
	}
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	if _, err := s.Up(ctx); err != nil {
		s.Close()
		b.close()
		return reply(started{Error: err.Error()})
	}
	p, err := newProxy(s)
	if err != nil {
		s.Close()
		b.close()
		return reply(started{Error: err.Error()})
	}
	srv = s
	door = b
	proxy = p
	return reply(started{Proxy: p.addr(), Credential: p.cred, IP: ipOf(s)})
}

func ipOf(s *tsnet.Server) string {
	v4, _ := s.TailscaleIPs()
	if v4.IsValid() {
		return v4.String()
	}
	return ""
}

// commonty_net_status returns json: whether the node is up, its address
// and name, the peers it can see.
//
//export commonty_net_status
func commonty_net_status() *C.char {
	mu.Lock()
	defer mu.Unlock()
	if srv == nil {
		return reply(status{State: "stopped"})
	}
	lc, err := srv.LocalClient()
	if err != nil {
		return reply(status{Error: err.Error()})
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	st, err := lc.Status(ctx)
	if err != nil {
		return reply(status{Error: err.Error()})
	}
	out := status{Running: st.BackendState == "Running", State: st.BackendState, IP: ipOf(srv)}
	if st.Self != nil {
		out.Name = st.Self.HostName
	}
	for _, p := range st.Peer {
		ip := ""
		if len(p.TailscaleIPs) > 0 {
			ip = p.TailscaleIPs[0].String()
		}
		out.Peers = append(out.Peers, peer{Name: p.HostName, IP: ip, Online: p.Online})
	}
	return reply(out)
}

// commonty_net_stop leaves the network for this run; the state stays for
// the next start.
//
//export commonty_net_stop
func commonty_net_stop() {
	mu.Lock()
	defer mu.Unlock()
	if proxy != nil {
		proxy.close()
		proxy = nil
	}
	if srv != nil {
		srv.Close()
		srv = nil
	}
	if door != nil {
		door.close()
		door = nil
	}
}

// commonty_net_free releases a string this library returned.
//
//export commonty_net_free
func commonty_net_free(p *C.char) {
	C.free(unsafe.Pointer(p))
}

func main() {}
