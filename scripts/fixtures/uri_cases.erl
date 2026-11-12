-module(uri_cases).
-export([cases/0]).
cases() ->
 Parse = [<<>>,<<"a/b">>,<<"HTTP://u:p@Example.COM:80/a?b#c">>,<<"//[::1]:443/x">>,<<"http://h:">>,<<"http://h:99999/">>,<<"foo:bar">>,<<"//">>,<<"?">>,<<"#">>,<<"http:///x">>,<<"a%zz">>,<<"a b">>,<<"http://h:xx/">>,<<"http://[no]/">>,<<"1:x">>,<<"/a?x?y#f">>,<<"a#x#y">>,<<"http://[:::1]/">>,<<"http://[1:2]/">>,<<"http://[1:2:3:4:5:6:7:8]/">>,<<"http://[::ffff:192.0.2.1]/">>,<<"http://[v1.a:b]/">>,<<"http://[v12.a:b]/">>],
 Compose = [#{},#{host=><<"h">>},#{path=><<"a b">>},#{path=>"é?",query=><<"a b">>},#{scheme=><<"HTTP">>,host=><<"H">>,path=><<>>},#{host=><<"h">>,path=><<"x">>},#{host=><<"::1">>,path=><<"/">>,port=>80},#{path=><<"x">>,extra=>1},#{path=><<"a%20b">>},#{path=><<"x">>,port=>80}],
 Norm = [<<"HTTP://H:80">>,<<"https://H:443/./a/../b/%7e/%2f">>,<<"../a/../b">>,<<"foo:/a//b/./c/../">>,<<"a%zz">>,<<"a%">>,<<"http://%65XAMPLE.com/%41%2f">>,<<"ftp://H:21">>,<<"http://h:81">>,<<"http://h:">>,<<"http://%2f/">>,<<"http:">>,#{scheme=><<"http">>,host=><<"H">>,path=><<>>}],
 Refs = [<<"g:h">>,<<"g">>,<<"./g">>,<<"g/">>,<<"/g">>,<<"//g">>,<<"?y">>,<<"g?y">>,<<"#s">>,<<"g#s">>,<<"g?y#s">>,<<";x">>,<<"g;x">>,<<"g;x?y#s">>,<<>>,<<".">>,<<"./">>,<<"..">>,<<"../">>,<<"../g">>,<<"../..">>,<<"../../">>,<<"../../g">>,<<"../../../g">>,<<"../../../../g">>,<<"/./g">>,<<"/../g">>,<<"g.">>,<<".g">>,<<"g..">>,<<"..g">>,<<"./../g">>,<<"./g/.">>,<<"g/./h">>,<<"g/../h">>,<<"g;x=1/./y">>,<<"g;x=1/../y">>,<<"g?y/./x">>,<<"g?y/../x">>,<<"g#s/./x">>,<<"g#s/../x">>,<<"http:g">>],
 [{parse,[U]} || U<-Parse] ++ [{parse,[binary_to_list(U)]} || U<-Parse] ++
 [{recompose,[M]} || M<-Compose] ++ [{normalize,[U]} || U<-Norm] ++
 [{normalize,[binary_to_list(U)]} || U<-Norm,is_binary(U)] ++
 [{resolve,[R,<<"http://a/b/c/d;p?q">>,O]} || R<-Refs,O<-[[],[return_map]]] ++
 [{resolve,["g",<<"http://h/a/b">>,[return_map]]},
  {resolve,[<<"g">>,"http://h/a/b",[return_map]]},
  {resolve,[<<"g">>,<<"relative">>,[return_map]]},
  {resolve,[<<"g">>,<<"http://h/">>,[bad]]},
  {resolve,[#{path=><<"g">>},#{scheme=>"http",host=>"h",path=>"/a/b"},[return_map]]}].
