package devupstream_test

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"forge-cloud/internal/devupstream"
	"forge-cloud/internal/testutil"
)

func TestFakeChatCompletions(t *testing.T) {
	s := devupstream.New(testutil.Logger())
	srv := httptest.NewServer(s)
	t.Cleanup(srv.Close)
	body := `{"model":"gpt-fake","messages":[{"role":"user","content":"ping"}]}`
	req, _ := http.NewRequest(http.MethodPost, srv.URL+"/v1/chat/completions", strings.NewReader(body))
	req.Header.Set("Authorization", "Bearer sk-test")
	req.Header.Set("Content-Type", "application/json")
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatal(err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status=%d", resp.StatusCode)
	}
}
