// The bridge on its own, for a box. A box's stock tailscaled cannot carry
// the control protocol through the front door, and on a box already on
// the owner's tailnet it has no route to the control box's address either:
// it steers its own control traffic around every tailnet, including the
// other one. This listens on loopback and carries it through the front
// door as a websocket, the way the app does.
package main

import (
	"flag"
	"log"

	"commonty.org/app/net/bridge"
)

func main() {
	listen := flag.String("listen", "127.0.0.1:41643", "where the box's tailscaled is pointed")
	control := flag.String("control", "", "the control server, through the front door")
	flag.Parse()
	if *control == "" {
		log.Fatal("-control is required")
	}
	b, err := bridge.New(*control, *listen)
	if err != nil {
		log.Fatal(err)
	}
	log.Printf("carrying %s to %s", b.URL(), *control)
	select {}
}
