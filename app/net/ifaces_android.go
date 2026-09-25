//go:build android

// Android forbids the netlink call Go's net.Interfaces makes (since API
// 30), and the engine cannot come up without an interface list. bionic's
// getifaddrs is the sanctioned way; this hands its answer to the engine.
package main

/*
#include <stdlib.h>
#include <ifaddrs.h>
#include <net/if.h>
#include <netinet/in.h>
#include <sys/socket.h>
#include <sys/ioctl.h>
#include <string.h>
#include <unistd.h>

static int if_index_of(const char *name) {
	return (int)if_nametoindex(name);
}

static int if_mtu_of(const char *name) {
	int fd = socket(AF_INET, SOCK_DGRAM, 0);
	if (fd < 0) return 1500;
	struct ifreq ifr;
	memset(&ifr, 0, sizeof ifr);
	strncpy(ifr.ifr_name, name, IFNAMSIZ - 1);
	int mtu = 1500;
	if (ioctl(fd, SIOCGIFMTU, &ifr) == 0) mtu = ifr.ifr_mtu;
	close(fd);
	return mtu;
}
*/
import "C"

import (
	"net"
	"unsafe"

	"tailscale.com/net/netmon"
)

func init() {
	netmon.RegisterInterfaceGetter(androidInterfaces)
}

func androidInterfaces() ([]netmon.Interface, error) {
	var head *C.struct_ifaddrs
	if rc, err := C.getifaddrs(&head); rc != 0 {
		return nil, err
	}
	defer C.freeifaddrs(head)
	byName := map[string]*netmon.Interface{}
	var order []string
	for p := head; p != nil; p = p.ifa_next {
		name := C.GoString(p.ifa_name)
		it, ok := byName[name]
		if !ok {
			cname := C.CString(name)
			it = &netmon.Interface{
				Interface: &net.Interface{
					Index: int(C.if_index_of(cname)),
					MTU:   int(C.if_mtu_of(cname)),
					Name:  name,
					Flags: flagsOf(uint32(p.ifa_flags)),
				},
				AltAddrs: []net.Addr{},
			}
			C.free(unsafe.Pointer(cname))
			byName[name] = it
			order = append(order, name)
		}
		if p.ifa_addr == nil {
			continue
		}
		switch p.ifa_addr.sa_family {
		case C.AF_INET:
			sa := (*C.struct_sockaddr_in)(unsafe.Pointer(p.ifa_addr))
			ip := make(net.IP, 4)
			copy(ip, (*[4]byte)(unsafe.Pointer(&sa.sin_addr))[:])
			mask := net.CIDRMask(32, 32)
			if p.ifa_netmask != nil {
				nm := (*C.struct_sockaddr_in)(unsafe.Pointer(p.ifa_netmask))
				mask = net.IPMask((*[4]byte)(unsafe.Pointer(&nm.sin_addr))[:])
			}
			it.AltAddrs = append(it.AltAddrs, &net.IPNet{IP: ip, Mask: mask})
		case C.AF_INET6:
			sa := (*C.struct_sockaddr_in6)(unsafe.Pointer(p.ifa_addr))
			ip := make(net.IP, 16)
			copy(ip, (*[16]byte)(unsafe.Pointer(&sa.sin6_addr))[:])
			mask := net.CIDRMask(128, 128)
			if p.ifa_netmask != nil {
				nm := (*C.struct_sockaddr_in6)(unsafe.Pointer(p.ifa_netmask))
				mask = net.IPMask((*[16]byte)(unsafe.Pointer(&nm.sin6_addr))[:])
			}
			it.AltAddrs = append(it.AltAddrs, &net.IPNet{IP: ip, Mask: mask})
		}
	}
	out := make([]netmon.Interface, 0, len(order))
	for _, n := range order {
		out = append(out, *byName[n])
	}
	return out, nil
}

func flagsOf(f uint32) net.Flags {
	var out net.Flags
	if f&C.IFF_UP != 0 {
		out |= net.FlagUp
	}
	if f&C.IFF_BROADCAST != 0 {
		out |= net.FlagBroadcast
	}
	if f&C.IFF_LOOPBACK != 0 {
		out |= net.FlagLoopback
	}
	if f&C.IFF_POINTOPOINT != 0 {
		out |= net.FlagPointToPoint
	}
	if f&C.IFF_MULTICAST != 0 {
		out |= net.FlagMulticast
	}
	if f&C.IFF_RUNNING != 0 {
		out |= net.FlagRunning
	}
	return out
}
