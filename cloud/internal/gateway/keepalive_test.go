package gateway

import (
	"context"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"forge-cloud/internal/gateway/convert"
)

type idleDeadlineConn struct {
	net.Conn
	idle time.Duration
}

func (c *idleDeadlineConn) Read(b []byte) (int, error) {
	_ = c.SetReadDeadline(time.Now().Add(c.idle))
	return c.Conn.Read(b)
}

func TestKeepalivePreventsIdleReadTimeout(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		c := &call{ctx: r.Context(), w: w, rc: http.NewResponseController(w), start: time.Now().Add(-time.Millisecond)}
		if !c.writeFrame([]byte("data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n")) {
			return
		}
		firstToken := c.firstToken
		body, writer := io.Pipe()
		go func() {
			defer writer.Close()
			select {
			case <-time.After(600 * time.Millisecond):
				_, _ = writer.Write([]byte("data: complete\n\ndata: [DONE]\n\n"))
			case <-r.Context().Done():
			}
		}()
		next, stop := c.streamEvents(body, true, 25*time.Millisecond)
		defer stop()
		for {
			ev, err := next()
			if err != nil || !c.writeFrame(convert.FormatSSE(ev.Event, ev.Data)) || string(ev.Data) == "[DONE]" {
				break
			}
		}
		if c.firstToken != firstToken {
			t.Error("keepalive changed first-token timing")
		}
	}))
	defer server.Close()
	transport := &http.Transport{DialContext: func(ctx context.Context, network, addr string) (net.Conn, error) {
		conn, err := (&net.Dialer{Timeout: time.Second}).DialContext(ctx, network, addr)
		if err != nil {
			return nil, err
		}
		return &idleDeadlineConn{Conn: conn, idle: 200 * time.Millisecond}, nil
	}}
	defer transport.CloseIdleConnections()
	client := &http.Client{Transport: transport, Timeout: 3 * time.Second}
	resp, err := client.Get(server.URL)
	if err != nil {
		t.Fatal(err)
	}
	defer resp.Body.Close()
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		t.Fatalf("idle timeout during model thought: %v", err)
	}
	if !strings.Contains(string(body), ": keep-alive\n\n") || !strings.Contains(string(body), "data: complete\n\ndata: [DONE]") {
		t.Fatalf("missing keepalive or completed answer: %s", body)
	}
}

func TestKeepalivePreservesErrorBeforeFirstFrame(t *testing.T) {
	w := httptest.NewRecorder()
	c := &call{ctx: context.Background(), w: w, rc: http.NewResponseController(w), start: time.Now()}
	body, writer := io.Pipe()
	go func() {
		time.Sleep(50 * time.Millisecond)
		_ = writer.CloseWithError(io.ErrUnexpectedEOF)
	}()
	next, stop := c.streamEvents(body, true, 5*time.Millisecond)
	defer stop()
	if _, err := next(); err != io.ErrUnexpectedEOF {
		t.Fatalf("read error=%v", err)
	}
	if c.started || w.Body.Len() != 0 || c.firstToken != 0 {
		t.Fatal("keepalive committed a response before the upstream's first frame")
	}
}
