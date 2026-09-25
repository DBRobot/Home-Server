// The proxy the Rust side talks to: a SOCKS5 server on localhost whose
// dialer resolves names through the network's own DNS first (the fleet's
// names live there, as records the control server hands out), and only
// then the system's. tsnet's built-in loopback proxy asks the system
// resolver, which answers a fleet name with an address that means nothing
// on this network.
package main

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"net"
	"strings"

	"golang.org/x/net/dns/dnsmessage"
	"tailscale.com/client/local"
	"tailscale.com/net/socks5"
	"tailscale.com/tsnet"
)

type proxyServer struct {
	ln   net.Listener
	cred string
}

func newProxy(s *tsnet.Server) (*proxyServer, error) {
	lc, err := s.LocalClient()
	if err != nil {
		return nil, err
	}
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		return nil, err
	}
	cred := hex.EncodeToString(b[:])
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		return nil, err
	}
	srv := &socks5.Server{
		Logf:     func(string, ...any) {},
		Username: "tsnet",
		Password: cred,
		Dialer: func(ctx context.Context, network, addr string) (net.Conn, error) {
			host, port, err := net.SplitHostPort(addr)
			if err != nil {
				return nil, err
			}
			if net.ParseIP(host) == nil {
				if ip, err := resolve(ctx, lc, host); err == nil {
					addr = net.JoinHostPort(ip, port)
				}
			}
			return s.Dial(ctx, network, addr)
		},
	}
	go func() { _ = srv.Serve(ln) }()
	return &proxyServer{ln: ln, cred: cred}, nil
}

func (p *proxyServer) addr() string { return p.ln.Addr().String() }
func (p *proxyServer) close()       { p.ln.Close() }

// resolve asks the engine's resolver for an A record: the network's names
// and its extra records answer here; anything else falls through to the
// system resolver in the dialer
func resolve(ctx context.Context, lc *local.Client, name string) (string, error) {
	raw, _, err := lc.QueryDNS(ctx, strings.TrimSuffix(name, ".")+".", "A")
	if err != nil {
		return "", err
	}
	var p dnsmessage.Parser
	if _, err := p.Start(raw); err != nil {
		return "", err
	}
	if err := p.SkipAllQuestions(); err != nil {
		return "", err
	}
	for {
		h, err := p.AnswerHeader()
		if errors.Is(err, dnsmessage.ErrSectionDone) {
			break
		}
		if err != nil {
			return "", err
		}
		if h.Type == dnsmessage.TypeA {
			r, err := p.AResource()
			if err != nil {
				return "", err
			}
			return net.IP(r.A[:]).String(), nil
		}
		if err := p.SkipAnswer(); err != nil {
			return "", err
		}
	}
	return "", fmt.Errorf("no A record for %s", name)
}
