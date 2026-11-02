import{A as e,D as t,M as n,T as r,V as i,k as a,w as o}from"./shared-Cx6TDTfW.js";import{i as s}from"./vendor-C6TRNjRf.js";import{g as c}from"./targets-gtreyB-C.js";function l(e){let t=e[0],n=e[1],r=e[2];return Math.sqrt(t*t+n*n+r*r)}function u(e,t){return e[0]=t[0],e[1]=t[1],e[2]=t[2],e}function d(e,t,n,r){return e[0]=t,e[1]=n,e[2]=r,e}function f(e,t,n){return e[0]=t[0]+n[0],e[1]=t[1]+n[1],e[2]=t[2]+n[2],e}function p(e,t,n){return e[0]=t[0]-n[0],e[1]=t[1]-n[1],e[2]=t[2]-n[2],e}function m(e,t,n){return e[0]=t[0]*n[0],e[1]=t[1]*n[1],e[2]=t[2]*n[2],e}function h(e,t,n){return e[0]=t[0]/n[0],e[1]=t[1]/n[1],e[2]=t[2]/n[2],e}function g(e,t,n){return e[0]=t[0]*n,e[1]=t[1]*n,e[2]=t[2]*n,e}function _(e,t){let n=t[0]-e[0],r=t[1]-e[1],i=t[2]-e[2];return Math.sqrt(n*n+r*r+i*i)}function v(e,t){let n=t[0]-e[0],r=t[1]-e[1],i=t[2]-e[2];return n*n+r*r+i*i}function y(e){let t=e[0],n=e[1],r=e[2];return t*t+n*n+r*r}function b(e,t){return e[0]=-t[0],e[1]=-t[1],e[2]=-t[2],e}function x(e,t){return e[0]=1/t[0],e[1]=1/t[1],e[2]=1/t[2],e}function S(e,t){let n=t[0],r=t[1],i=t[2],a=n*n+r*r+i*i;return a>0&&(a=1/Math.sqrt(a)),e[0]=t[0]*a,e[1]=t[1]*a,e[2]=t[2]*a,e}function C(e,t){return e[0]*t[0]+e[1]*t[1]+e[2]*t[2]}function w(e,t,n){let r=t[0],i=t[1],a=t[2],o=n[0],s=n[1],c=n[2];return e[0]=i*c-a*s,e[1]=a*o-r*c,e[2]=r*s-i*o,e}function T(e,t,n,r){let i=t[0],a=t[1],o=t[2];return e[0]=i+r*(n[0]-i),e[1]=a+r*(n[1]-a),e[2]=o+r*(n[2]-o),e}function E(e,t,n,r,i){let a=Math.exp(-r*i),o=t[0],s=t[1],c=t[2];return e[0]=n[0]+(o-n[0])*a,e[1]=n[1]+(s-n[1])*a,e[2]=n[2]+(c-n[2])*a,e}function D(e,t,n){let r=t[0],i=t[1],a=t[2],o=n[3]*r+n[7]*i+n[11]*a+n[15];return o||=1,e[0]=(n[0]*r+n[4]*i+n[8]*a+n[12])/o,e[1]=(n[1]*r+n[5]*i+n[9]*a+n[13])/o,e[2]=(n[2]*r+n[6]*i+n[10]*a+n[14])/o,e}function O(e,t,n){let r=t[0],i=t[1],a=t[2],o=n[3]*r+n[7]*i+n[11]*a+n[15];return o||=1,e[0]=(n[0]*r+n[4]*i+n[8]*a)/o,e[1]=(n[1]*r+n[5]*i+n[9]*a)/o,e[2]=(n[2]*r+n[6]*i+n[10]*a)/o,e}function k(e,t,n){let r=t[0],i=t[1],a=t[2];return e[0]=r*n[0]+i*n[3]+a*n[6],e[1]=r*n[1]+i*n[4]+a*n[7],e[2]=r*n[2]+i*n[5]+a*n[8],e}function A(e,t,n){let r=t[0],i=t[1],a=t[2],o=n[0],s=n[1],c=n[2],l=n[3],u=s*a-c*i,d=c*r-o*a,f=o*i-s*r,p=s*f-c*d,m=c*u-o*f,h=o*d-s*u,g=l*2;return u*=g,d*=g,f*=g,p*=2,m*=2,h*=2,e[0]=r+u+p,e[1]=i+d+m,e[2]=a+f+h,e}var j=(function(){let e=[0,0,0],t=[0,0,0];return function(n,r){u(e,n),u(t,r),S(e,e),S(t,t);let i=C(e,t);return i>1?0:i<-1?Math.PI:Math.acos(i)}})();function M(e,t){return e[0]===t[0]&&e[1]===t[1]&&e[2]===t[2]}var N=class e extends Array{constructor(e=0,t=e,n=e){return super(e,t,n),this}get x(){return this[0]}get y(){return this[1]}get z(){return this[2]}set x(e){this[0]=e}set y(e){this[1]=e}set z(e){this[2]=e}set(e,t=e,n=e){return e.length?this.copy(e):(d(this,e,t,n),this)}copy(e){return u(this,e),this}add(e,t){return t?f(this,e,t):f(this,this,e),this}sub(e,t){return t?p(this,e,t):p(this,this,e),this}multiply(e){return e.length?m(this,this,e):g(this,this,e),this}divide(e){return e.length?h(this,this,e):g(this,this,1/e),this}inverse(e=this){return x(this,e),this}len(){return l(this)}distance(e){return e?_(this,e):l(this)}squaredLen(){return y(this)}squaredDistance(e){return e?v(this,e):y(this)}negate(e=this){return b(this,e),this}cross(e,t){return t?w(this,e,t):w(this,this,e),this}scale(e){return g(this,this,e),this}normalize(){return S(this,this),this}dot(e){return C(this,e)}equals(e){return M(this,e)}applyMatrix3(e){return k(this,this,e),this}applyMatrix4(e){return D(this,this,e),this}scaleRotateMatrix4(e){return O(this,this,e),this}applyQuaternion(e){return A(this,this,e),this}angle(e){return j(this,e)}lerp(e,t){return T(this,this,e,t),this}smoothLerp(e,t,n){return E(this,this,e,t,n),this}clone(){return new e(this[0],this[1],this[2])}fromArray(e,t=0){return this[0]=e[t],this[1]=e[t+1],this[2]=e[t+2],this}toArray(e=[],t=0){return e[t]=this[0],e[t+1]=this[1],e[t+2]=this[2],e}transformDirection(e){let t=this[0],n=this[1],r=this[2];return this[0]=e[0]*t+e[4]*n+e[8]*r,this[1]=e[1]*t+e[5]*n+e[9]*r,this[2]=e[2]*t+e[6]*n+e[10]*r,this.normalize()}},P=new N,F=1,I=1,L=!1,ee=class{constructor(e,t={}){e.canvas||console.error(`gl not passed as first argument to Geometry`),this.gl=e,this.attributes=t,this.id=F++,this.VAOs={},this.drawRange={start:0,count:0},this.instancedCount=0,this.gl.renderer.bindVertexArray(null),this.gl.renderer.currentGeometry=null,this.glState=this.gl.renderer.state;for(let e in t)this.addAttribute(e,t[e])}addAttribute(e,t){if(this.attributes[e]=t,t.id=I++,t.size=t.size||1,t.type=t.type||(t.data.constructor===Float32Array?this.gl.FLOAT:t.data.constructor===Uint16Array?this.gl.UNSIGNED_SHORT:this.gl.UNSIGNED_INT),t.target=e===`index`?this.gl.ELEMENT_ARRAY_BUFFER:this.gl.ARRAY_BUFFER,t.normalized=t.normalized||!1,t.stride=t.stride||0,t.offset=t.offset||0,t.count=t.count||(t.stride?t.data.byteLength/t.stride:t.data.length/t.size),t.divisor=t.instanced||0,t.needsUpdate=!1,t.usage=t.usage||this.gl.STATIC_DRAW,t.buffer||this.updateAttribute(t),t.divisor){if(this.isInstanced=!0,this.instancedCount&&this.instancedCount!==t.count*t.divisor)return console.warn(`geometry has multiple instanced buffers of different length`),this.instancedCount=Math.min(this.instancedCount,t.count*t.divisor);this.instancedCount=t.count*t.divisor}else e===`index`?this.drawRange.count=t.count:this.attributes.index||(this.drawRange.count=Math.max(this.drawRange.count,t.count))}updateAttribute(e){let t=!e.buffer;t&&(e.buffer=this.gl.createBuffer()),this.glState.boundBuffer!==e.buffer&&(this.gl.bindBuffer(e.target,e.buffer),this.glState.boundBuffer=e.buffer),t?this.gl.bufferData(e.target,e.data,e.usage):this.gl.bufferSubData(e.target,0,e.data),e.needsUpdate=!1}setIndex(e){this.addAttribute(`index`,e)}setDrawRange(e,t){this.drawRange.start=e,this.drawRange.count=t}setInstancedCount(e){this.instancedCount=e}createVAO(e){this.VAOs[e.attributeOrder]=this.gl.renderer.createVertexArray(),this.gl.renderer.bindVertexArray(this.VAOs[e.attributeOrder]),this.bindAttributes(e)}bindAttributes(e){e.attributeLocations.forEach((e,{name:t,type:n})=>{if(!this.attributes[t]){console.warn(`active attribute ${t} not being supplied`);return}let r=this.attributes[t];this.gl.bindBuffer(r.target,r.buffer),this.glState.boundBuffer=r.buffer;let i=1;n===35674&&(i=2),n===35675&&(i=3),n===35676&&(i=4);let a=r.size/i,o=i===1?0:i*i*4,s=i===1?0:i*4;for(let t=0;t<i;t++)this.gl.vertexAttribPointer(e+t,a,r.type,r.normalized,r.stride+o,r.offset+t*s),this.gl.enableVertexAttribArray(e+t),this.gl.renderer.vertexAttribDivisor(e+t,r.divisor)}),this.attributes.index&&this.gl.bindBuffer(this.gl.ELEMENT_ARRAY_BUFFER,this.attributes.index.buffer)}draw({program:e,mode:t=this.gl.TRIANGLES}){this.gl.renderer.currentGeometry!==`${this.id}_${e.attributeOrder}`&&(this.VAOs[e.attributeOrder]||this.createVAO(e),this.gl.renderer.bindVertexArray(this.VAOs[e.attributeOrder]),this.gl.renderer.currentGeometry=`${this.id}_${e.attributeOrder}`),e.attributeLocations.forEach((e,{name:t})=>{let n=this.attributes[t];n.needsUpdate&&this.updateAttribute(n)});let n=2;this.attributes.index?.type===this.gl.UNSIGNED_INT&&(n=4),this.isInstanced?this.attributes.index?this.gl.renderer.drawElementsInstanced(t,this.drawRange.count,this.attributes.index.type,this.attributes.index.offset+this.drawRange.start*n,this.instancedCount):this.gl.renderer.drawArraysInstanced(t,this.drawRange.start,this.drawRange.count,this.instancedCount):this.attributes.index?this.gl.drawElements(t,this.drawRange.count,this.attributes.index.type,this.attributes.index.offset+this.drawRange.start*n):this.gl.drawArrays(t,this.drawRange.start,this.drawRange.count)}getPosition(){let e=this.attributes.position;if(e.data)return e;if(!L)return console.warn(`No position buffer data found to compute bounds`),L=!0}computeBoundingBox(e){e||=this.getPosition();let t=e.data,n=e.size;this.bounds||={min:new N,max:new N,center:new N,scale:new N,radius:1/0};let r=this.bounds.min,i=this.bounds.max,a=this.bounds.center,o=this.bounds.scale;r.set(1/0),i.set(-1/0);for(let e=0,a=t.length;e<a;e+=n){let n=t[e],a=t[e+1],o=t[e+2];r.x=Math.min(n,r.x),r.y=Math.min(a,r.y),r.z=Math.min(o,r.z),i.x=Math.max(n,i.x),i.y=Math.max(a,i.y),i.z=Math.max(o,i.z)}o.sub(i,r),a.add(r,i).divide(2)}computeBoundingSphere(e){e||=this.getPosition();let t=e.data,n=e.size;this.bounds||this.computeBoundingBox(e);let r=0;for(let e=0,i=t.length;e<i;e+=n)P.fromArray(t,e),r=Math.max(r,this.bounds.center.squaredDistance(P));this.bounds.radius=Math.sqrt(r)}remove(){for(let e in this.VAOs)this.gl.renderer.deleteVertexArray(this.VAOs[e]),delete this.VAOs[e];for(let e in this.attributes)this.gl.deleteBuffer(this.attributes[e].buffer),delete this.attributes[e]}},R=1,z={},te=class{constructor(e,{vertex:t,fragment:n,uniforms:r={},transparent:i=!1,cullFace:a=e.BACK,frontFace:o=e.CCW,depthTest:s=!0,depthWrite:c=!0,depthFunc:l=e.LEQUAL}={}){e.canvas||console.error(`gl not passed as first argument to Program`),this.gl=e,this.uniforms=r,this.id=R++,t||console.warn(`vertex shader not supplied`),n||console.warn(`fragment shader not supplied`),this.transparent=i,this.cullFace=a,this.frontFace=o,this.depthTest=s,this.depthWrite=c,this.depthFunc=l,this.blendFunc={},this.blendEquation={},this.stencilFunc={},this.stencilOp={},this.transparent&&!this.blendFunc.src&&(this.gl.renderer.premultipliedAlpha?this.setBlendFunc(this.gl.ONE,this.gl.ONE_MINUS_SRC_ALPHA):this.setBlendFunc(this.gl.SRC_ALPHA,this.gl.ONE_MINUS_SRC_ALPHA)),this.vertexShader=e.createShader(e.VERTEX_SHADER),this.fragmentShader=e.createShader(e.FRAGMENT_SHADER),this.program=e.createProgram(),e.attachShader(this.program,this.vertexShader),e.attachShader(this.program,this.fragmentShader),this.setShaders({vertex:t,fragment:n})}setShaders({vertex:e,fragment:t}){if(e&&(this.gl.shaderSource(this.vertexShader,e),this.gl.compileShader(this.vertexShader),this.gl.getShaderInfoLog(this.vertexShader)!==``&&console.warn(`${this.gl.getShaderInfoLog(this.vertexShader)}\nVertex Shader\n${V(e)}`)),t&&(this.gl.shaderSource(this.fragmentShader,t),this.gl.compileShader(this.fragmentShader),this.gl.getShaderInfoLog(this.fragmentShader)!==``&&console.warn(`${this.gl.getShaderInfoLog(this.fragmentShader)}\nFragment Shader\n${V(t)}`)),this.gl.linkProgram(this.program),!this.gl.getProgramParameter(this.program,this.gl.LINK_STATUS))return console.warn(this.gl.getProgramInfoLog(this.program));this.uniformLocations=new Map;let n=this.gl.getProgramParameter(this.program,this.gl.ACTIVE_UNIFORMS);for(let e=0;e<n;e++){let t=this.gl.getActiveUniform(this.program,e);this.uniformLocations.set(t,this.gl.getUniformLocation(this.program,t.name));let n=t.name.match(/(\w+)/g);t.uniformName=n[0],t.nameComponents=n.slice(1)}this.attributeLocations=new Map;let r=[],i=this.gl.getProgramParameter(this.program,this.gl.ACTIVE_ATTRIBUTES);for(let e=0;e<i;e++){let t=this.gl.getActiveAttrib(this.program,e),n=this.gl.getAttribLocation(this.program,t.name);n!==-1&&(r[n]=t.name,this.attributeLocations.set(t,n))}this.attributeOrder=r.join(``)}setBlendFunc(e,t,n,r){this.blendFunc.src=e,this.blendFunc.dst=t,this.blendFunc.srcAlpha=n,this.blendFunc.dstAlpha=r,e&&(this.transparent=!0)}setBlendEquation(e,t){this.blendEquation.modeRGB=e,this.blendEquation.modeAlpha=t}setStencilFunc(e,t,n){this.stencilRef=t,this.stencilFunc.func=e,this.stencilFunc.ref=t,this.stencilFunc.mask=n}setStencilOp(e,t,n){this.stencilOp.stencilFail=e,this.stencilOp.depthFail=t,this.stencilOp.depthPass=n}applyState(){this.depthTest?this.gl.renderer.enable(this.gl.DEPTH_TEST):this.gl.renderer.disable(this.gl.DEPTH_TEST),this.cullFace?this.gl.renderer.enable(this.gl.CULL_FACE):this.gl.renderer.disable(this.gl.CULL_FACE),this.blendFunc.src?this.gl.renderer.enable(this.gl.BLEND):this.gl.renderer.disable(this.gl.BLEND),this.cullFace&&this.gl.renderer.setCullFace(this.cullFace),this.gl.renderer.setFrontFace(this.frontFace),this.gl.renderer.setDepthMask(this.depthWrite),this.gl.renderer.setDepthFunc(this.depthFunc),this.blendFunc.src&&this.gl.renderer.setBlendFunc(this.blendFunc.src,this.blendFunc.dst,this.blendFunc.srcAlpha,this.blendFunc.dstAlpha),this.gl.renderer.setBlendEquation(this.blendEquation.modeRGB,this.blendEquation.modeAlpha),this.stencilFunc.func||this.stencilOp.stencilFail?this.gl.renderer.enable(this.gl.STENCIL_TEST):this.gl.renderer.disable(this.gl.STENCIL_TEST),this.gl.renderer.setStencilFunc(this.stencilFunc.func,this.stencilFunc.ref,this.stencilFunc.mask),this.gl.renderer.setStencilOp(this.stencilOp.stencilFail,this.stencilOp.depthFail,this.stencilOp.depthPass)}use({flipFaces:e=!1}={}){let t=-1;this.gl.renderer.state.currentProgram!==this.id&&(this.gl.useProgram(this.program),this.gl.renderer.state.currentProgram=this.id),this.uniformLocations.forEach((e,n)=>{let r=this.uniforms[n.uniformName];for(let e of n.nameComponents){if(!r)break;if(e in r)r=r[e];else if(Array.isArray(r.value))break;else{r=void 0;break}}if(!r)return G(`Active uniform ${n.name} has not been supplied`);if(r&&r.value===void 0)return G(`${n.name} uniform is missing a value parameter`);if(r.value.texture)return t+=1,r.value.update(t),B(this.gl,n.type,e,t);if(r.value.length&&r.value[0].texture){let i=[];return r.value.forEach(e=>{t+=1,e.update(t),i.push(t)}),B(this.gl,n.type,e,i)}B(this.gl,n.type,e,r.value)}),this.applyState(),e&&this.gl.renderer.setFrontFace(this.frontFace===this.gl.CCW?this.gl.CW:this.gl.CCW)}remove(){this.gl.deleteProgram(this.program)}};function B(e,t,n,r){r=r.length?ne(r):r;let i=e.renderer.state.uniformLocations.get(n);if(r.length){if(i===void 0||i.length!==r.length)e.renderer.state.uniformLocations.set(n,r.slice(0));else{if(H(i,r))return;i.set?i.set(r):U(i,r),e.renderer.state.uniformLocations.set(n,i)}}else{if(i===r)return;e.renderer.state.uniformLocations.set(n,r)}switch(t){case 5126:return r.length?e.uniform1fv(n,r):e.uniform1f(n,r);case 35664:return e.uniform2fv(n,r);case 35665:return e.uniform3fv(n,r);case 35666:return e.uniform4fv(n,r);case 35670:case 5124:case 35678:case 36306:case 35680:case 36289:return r.length?e.uniform1iv(n,r):e.uniform1i(n,r);case 35671:case 35667:return e.uniform2iv(n,r);case 35672:case 35668:return e.uniform3iv(n,r);case 35673:case 35669:return e.uniform4iv(n,r);case 35674:return e.uniformMatrix2fv(n,!1,r);case 35675:return e.uniformMatrix3fv(n,!1,r);case 35676:return e.uniformMatrix4fv(n,!1,r)}}function V(e){let t=e.split(`
`);for(let e=0;e<t.length;e++)t[e]=e+1+`: `+t[e];return t.join(`
`)}function ne(e){let t=e.length,n=e[0].length;if(n===void 0)return e;let r=t*n,i=z[r];i||(z[r]=i=new Float32Array(r));for(let r=0;r<t;r++)i.set(e[r],r*n);return i}function H(e,t){if(e.length!==t.length)return!1;for(let n=0,r=e.length;n<r;n++)if(e[n]!==t[n])return!1;return!0}function U(e,t){for(let n=0,r=e.length;n<r;n++)e[n]=t[n]}var W=0;function G(e){W>100||(console.warn(e),W++,W>100&&console.warn(`More than 100 program warnings - stopping logs.`))}var K=new N,re=1,ie=class{constructor({canvas:e=document.createElement(`canvas`),width:t=300,height:n=150,dpr:r=1,alpha:i=!1,depth:a=!0,stencil:o=!1,antialias:s=!1,premultipliedAlpha:c=!1,preserveDrawingBuffer:l=!1,powerPreference:u=`default`,autoClear:d=!0,webgl:f=2}={}){let p={alpha:i,depth:a,stencil:o,antialias:s,premultipliedAlpha:c,preserveDrawingBuffer:l,powerPreference:u};this.dpr=r,this.alpha=i,this.color=!0,this.depth=a,this.stencil=o,this.premultipliedAlpha=c,this.autoClear=d,this.id=re++,f===2&&(this.gl=e.getContext(`webgl2`,p)),this.isWebgl2=!!this.gl,this.gl||=e.getContext(`webgl`,p),this.gl||console.error(`unable to create webgl context`),this.gl.renderer=this,this.setSize(t,n),this.state={},this.state.blendFunc={src:this.gl.ONE,dst:this.gl.ZERO},this.state.blendEquation={modeRGB:this.gl.FUNC_ADD},this.state.cullFace=!1,this.state.frontFace=this.gl.CCW,this.state.depthMask=!0,this.state.depthFunc=this.gl.LEQUAL,this.state.premultiplyAlpha=!1,this.state.flipY=!1,this.state.unpackAlignment=4,this.state.framebuffer=null,this.state.viewport={x:0,y:0,width:null,height:null},this.state.textureUnits=[],this.state.activeTextureUnit=0,this.state.boundBuffer=null,this.state.uniformLocations=new Map,this.state.currentProgram=null,this.extensions={},this.isWebgl2?(this.getExtension(`EXT_color_buffer_float`),this.getExtension(`OES_texture_float_linear`)):(this.getExtension(`OES_texture_float`),this.getExtension(`OES_texture_float_linear`),this.getExtension(`OES_texture_half_float`),this.getExtension(`OES_texture_half_float_linear`),this.getExtension(`OES_element_index_uint`),this.getExtension(`OES_standard_derivatives`),this.getExtension(`EXT_sRGB`),this.getExtension(`WEBGL_depth_texture`),this.getExtension(`WEBGL_draw_buffers`)),this.getExtension(`WEBGL_compressed_texture_astc`),this.getExtension(`EXT_texture_compression_bptc`),this.getExtension(`WEBGL_compressed_texture_s3tc`),this.getExtension(`WEBGL_compressed_texture_etc1`),this.getExtension(`WEBGL_compressed_texture_pvrtc`),this.getExtension(`WEBKIT_WEBGL_compressed_texture_pvrtc`),this.vertexAttribDivisor=this.getExtension(`ANGLE_instanced_arrays`,`vertexAttribDivisor`,`vertexAttribDivisorANGLE`),this.drawArraysInstanced=this.getExtension(`ANGLE_instanced_arrays`,`drawArraysInstanced`,`drawArraysInstancedANGLE`),this.drawElementsInstanced=this.getExtension(`ANGLE_instanced_arrays`,`drawElementsInstanced`,`drawElementsInstancedANGLE`),this.createVertexArray=this.getExtension(`OES_vertex_array_object`,`createVertexArray`,`createVertexArrayOES`),this.bindVertexArray=this.getExtension(`OES_vertex_array_object`,`bindVertexArray`,`bindVertexArrayOES`),this.deleteVertexArray=this.getExtension(`OES_vertex_array_object`,`deleteVertexArray`,`deleteVertexArrayOES`),this.drawBuffers=this.getExtension(`WEBGL_draw_buffers`,`drawBuffers`,`drawBuffersWEBGL`),this.parameters={},this.parameters.maxTextureUnits=this.gl.getParameter(this.gl.MAX_COMBINED_TEXTURE_IMAGE_UNITS),this.parameters.maxAnisotropy=this.getExtension(`EXT_texture_filter_anisotropic`)?this.gl.getParameter(this.getExtension(`EXT_texture_filter_anisotropic`).MAX_TEXTURE_MAX_ANISOTROPY_EXT):0}setSize(e,t){this.width=e,this.height=t,this.gl.canvas.width=e*this.dpr,this.gl.canvas.height=t*this.dpr,this.gl.canvas.style&&Object.assign(this.gl.canvas.style,{width:e+`px`,height:t+`px`})}setViewport(e,t,n=0,r=0){(this.state.viewport.width!==e||this.state.viewport.height!==t)&&(this.state.viewport.width=e,this.state.viewport.height=t,this.state.viewport.x=n,this.state.viewport.y=r,this.gl.viewport(n,r,e,t))}setScissor(e,t,n=0,r=0){this.gl.scissor(n,r,e,t)}enable(e){this.state[e]!==!0&&(this.gl.enable(e),this.state[e]=!0)}disable(e){this.state[e]!==!1&&(this.gl.disable(e),this.state[e]=!1)}setBlendFunc(e,t,n,r){(this.state.blendFunc.src!==e||this.state.blendFunc.dst!==t||this.state.blendFunc.srcAlpha!==n||this.state.blendFunc.dstAlpha!==r)&&(this.state.blendFunc.src=e,this.state.blendFunc.dst=t,this.state.blendFunc.srcAlpha=n,this.state.blendFunc.dstAlpha=r,n===void 0?this.gl.blendFunc(e,t):this.gl.blendFuncSeparate(e,t,n,r))}setBlendEquation(e,t){e||=this.gl.FUNC_ADD,(this.state.blendEquation.modeRGB!==e||this.state.blendEquation.modeAlpha!==t)&&(this.state.blendEquation.modeRGB=e,this.state.blendEquation.modeAlpha=t,t===void 0?this.gl.blendEquation(e):this.gl.blendEquationSeparate(e,t))}setCullFace(e){this.state.cullFace!==e&&(this.state.cullFace=e,this.gl.cullFace(e))}setFrontFace(e){this.state.frontFace!==e&&(this.state.frontFace=e,this.gl.frontFace(e))}setDepthMask(e){this.state.depthMask!==e&&(this.state.depthMask=e,this.gl.depthMask(e))}setDepthFunc(e){this.state.depthFunc!==e&&(this.state.depthFunc=e,this.gl.depthFunc(e))}setStencilMask(e){this.state.stencilMask!==e&&(this.state.stencilMask=e,this.gl.stencilMask(e))}setStencilFunc(e,t,n){(this.state.stencilFunc!==e||this.state.stencilRef!==t||this.state.stencilFuncMask!==n)&&(this.state.stencilFunc=e||this.gl.ALWAYS,this.state.stencilRef=t||0,this.state.stencilFuncMask=n||0,this.gl.stencilFunc(e||this.gl.ALWAYS,t||0,n||0))}setStencilOp(e,t,n){(this.state.stencilFail!==e||this.state.stencilDepthFail!==t||this.state.stencilDepthPass!==n)&&(this.state.stencilFail=e,this.state.stencilDepthFail=t,this.state.stencilDepthPass=n,this.gl.stencilOp(e,t,n))}activeTexture(e){this.state.activeTextureUnit!==e&&(this.state.activeTextureUnit=e,this.gl.activeTexture(this.gl.TEXTURE0+e))}bindFramebuffer({target:e=this.gl.FRAMEBUFFER,buffer:t=null}={}){this.state.framebuffer!==t&&(this.state.framebuffer=t,this.gl.bindFramebuffer(e,t))}getExtension(e,t,n){return t&&this.gl[t]?this.gl[t].bind(this.gl):(this.extensions[e]||(this.extensions[e]=this.gl.getExtension(e)),t?this.extensions[e]?this.extensions[e][n].bind(this.extensions[e]):null:this.extensions[e])}sortOpaque(e,t){return e.renderOrder===t.renderOrder?e.program.id===t.program.id?e.zDepth===t.zDepth?t.id-e.id:e.zDepth-t.zDepth:e.program.id-t.program.id:e.renderOrder-t.renderOrder}sortTransparent(e,t){return e.renderOrder===t.renderOrder?e.zDepth===t.zDepth?t.id-e.id:t.zDepth-e.zDepth:e.renderOrder-t.renderOrder}sortUI(e,t){return e.renderOrder===t.renderOrder?e.program.id===t.program.id?t.id-e.id:e.program.id-t.program.id:e.renderOrder-t.renderOrder}getRenderList({scene:e,camera:t,frustumCull:n,sort:r}){let i=[];if(t&&n&&t.updateFrustum(),e.traverse(e=>{if(!e.visible)return!0;e.draw&&(n&&e.frustumCulled&&t&&!t.frustumIntersectsMesh(e)||i.push(e))}),r){let e=[],n=[],r=[];i.forEach(i=>{i.program.transparent?i.program.depthTest?n.push(i):r.push(i):e.push(i),i.zDepth=0,i.renderOrder===0&&i.program.depthTest&&t&&(i.worldMatrix.getTranslation(K),K.applyMatrix4(t.projectionViewMatrix),i.zDepth=K.z)}),e.sort(this.sortOpaque),n.sort(this.sortTransparent),r.sort(this.sortUI),i=e.concat(n,r)}return i}render({scene:e,camera:t,target:n=null,update:r=!0,sort:i=!0,frustumCull:a=!0,clear:o}){n===null?(this.bindFramebuffer(),this.setViewport(this.width*this.dpr,this.height*this.dpr)):(this.bindFramebuffer(n),this.setViewport(n.width,n.height)),(o||this.autoClear&&o!==!1)&&(this.depth&&(!n||n.depth)&&(this.enable(this.gl.DEPTH_TEST),this.setDepthMask(!0)),(this.stencil||!n||n.stencil)&&(this.enable(this.gl.STENCIL_TEST),this.setStencilMask(255)),this.gl.clear((this.color?this.gl.COLOR_BUFFER_BIT:0)|(this.depth?this.gl.DEPTH_BUFFER_BIT:0)|(this.stencil?this.gl.STENCIL_BUFFER_BIT:0))),r&&e.updateMatrixWorld(),t&&t.updateMatrixWorld(),this.getRenderList({scene:e,camera:t,frustumCull:a,sort:i}).forEach(e=>{e.draw({camera:t})})}};function ae(e,t){return e[0]=t[0],e[1]=t[1],e[2]=t[2],e[3]=t[3],e}function oe(e,t,n,r,i){return e[0]=t,e[1]=n,e[2]=r,e[3]=i,e}function q(e,t){let n=t[0],r=t[1],i=t[2],a=t[3],o=n*n+r*r+i*i+a*a;return o>0&&(o=1/Math.sqrt(o)),e[0]=n*o,e[1]=r*o,e[2]=i*o,e[3]=a*o,e}function J(e,t){return e[0]*t[0]+e[1]*t[1]+e[2]*t[2]+e[3]*t[3]}function se(e){return e[0]=0,e[1]=0,e[2]=0,e[3]=1,e}function ce(e,t,n){n*=.5;let r=Math.sin(n);return e[0]=r*t[0],e[1]=r*t[1],e[2]=r*t[2],e[3]=Math.cos(n),e}function le(e,t,n){let r=t[0],i=t[1],a=t[2],o=t[3],s=n[0],c=n[1],l=n[2],u=n[3];return e[0]=r*u+o*s+i*l-a*c,e[1]=i*u+o*c+a*s-r*l,e[2]=a*u+o*l+r*c-i*s,e[3]=o*u-r*s-i*c-a*l,e}function ue(e,t,n){n*=.5;let r=t[0],i=t[1],a=t[2],o=t[3],s=Math.sin(n),c=Math.cos(n);return e[0]=r*c+o*s,e[1]=i*c+a*s,e[2]=a*c-i*s,e[3]=o*c-r*s,e}function de(e,t,n){n*=.5;let r=t[0],i=t[1],a=t[2],o=t[3],s=Math.sin(n),c=Math.cos(n);return e[0]=r*c-a*s,e[1]=i*c+o*s,e[2]=a*c+r*s,e[3]=o*c-i*s,e}function fe(e,t,n){n*=.5;let r=t[0],i=t[1],a=t[2],o=t[3],s=Math.sin(n),c=Math.cos(n);return e[0]=r*c+i*s,e[1]=i*c-r*s,e[2]=a*c+o*s,e[3]=o*c-a*s,e}function pe(e,t,n,r){let i=t[0],a=t[1],o=t[2],s=t[3],c=n[0],l=n[1],u=n[2],d=n[3],f,p,m,h,g;return p=i*c+a*l+o*u+s*d,p<0&&(p=-p,c=-c,l=-l,u=-u,d=-d),1-p>1e-6?(f=Math.acos(p),m=Math.sin(f),h=Math.sin((1-r)*f)/m,g=Math.sin(r*f)/m):(h=1-r,g=r),e[0]=h*i+g*c,e[1]=h*a+g*l,e[2]=h*o+g*u,e[3]=h*s+g*d,e}function me(e,t){let n=t[0],r=t[1],i=t[2],a=t[3],o=n*n+r*r+i*i+a*a,s=o?1/o:0;return e[0]=-n*s,e[1]=-r*s,e[2]=-i*s,e[3]=a*s,e}function he(e,t){return e[0]=-t[0],e[1]=-t[1],e[2]=-t[2],e[3]=t[3],e}function ge(e,t){let n=t[0]+t[4]+t[8],r;if(n>0)r=Math.sqrt(n+1),e[3]=.5*r,r=.5/r,e[0]=(t[5]-t[7])*r,e[1]=(t[6]-t[2])*r,e[2]=(t[1]-t[3])*r;else{let n=0;t[4]>t[0]&&(n=1),t[8]>t[n*3+n]&&(n=2);let i=(n+1)%3,a=(n+2)%3;r=Math.sqrt(t[n*3+n]-t[i*3+i]-t[a*3+a]+1),e[n]=.5*r,r=.5/r,e[3]=(t[i*3+a]-t[a*3+i])*r,e[i]=(t[i*3+n]+t[n*3+i])*r,e[a]=(t[a*3+n]+t[n*3+a])*r}return e}function Y(e,t,n=`YXZ`){let r=Math.sin(t[0]*.5),i=Math.cos(t[0]*.5),a=Math.sin(t[1]*.5),o=Math.cos(t[1]*.5),s=Math.sin(t[2]*.5),c=Math.cos(t[2]*.5);return n===`XYZ`?(e[0]=r*o*c+i*a*s,e[1]=i*a*c-r*o*s,e[2]=i*o*s+r*a*c,e[3]=i*o*c-r*a*s):n===`YXZ`?(e[0]=r*o*c+i*a*s,e[1]=i*a*c-r*o*s,e[2]=i*o*s-r*a*c,e[3]=i*o*c+r*a*s):n===`ZXY`?(e[0]=r*o*c-i*a*s,e[1]=i*a*c+r*o*s,e[2]=i*o*s+r*a*c,e[3]=i*o*c-r*a*s):n===`ZYX`?(e[0]=r*o*c-i*a*s,e[1]=i*a*c+r*o*s,e[2]=i*o*s-r*a*c,e[3]=i*o*c+r*a*s):n===`YZX`?(e[0]=r*o*c+i*a*s,e[1]=i*a*c+r*o*s,e[2]=i*o*s-r*a*c,e[3]=i*o*c-r*a*s):n===`XZY`&&(e[0]=r*o*c-i*a*s,e[1]=i*a*c-r*o*s,e[2]=i*o*s+r*a*c,e[3]=i*o*c+r*a*s),e}var X=ae,_e=oe,ve=J,ye=q,be=class extends Array{constructor(e=0,t=0,n=0,r=1){super(e,t,n,r),this.onChange=()=>{},this._target=this;let i=[`0`,`1`,`2`,`3`];return new Proxy(this,{set(e,t){let n=Reflect.set(...arguments);return n&&i.includes(t)&&e.onChange(),n}})}get x(){return this[0]}get y(){return this[1]}get z(){return this[2]}get w(){return this[3]}set x(e){this._target[0]=e,this.onChange()}set y(e){this._target[1]=e,this.onChange()}set z(e){this._target[2]=e,this.onChange()}set w(e){this._target[3]=e,this.onChange()}identity(){return se(this._target),this.onChange(),this}set(e,t,n,r){return e.length?this.copy(e):(_e(this._target,e,t,n,r),this.onChange(),this)}rotateX(e){return ue(this._target,this._target,e),this.onChange(),this}rotateY(e){return de(this._target,this._target,e),this.onChange(),this}rotateZ(e){return fe(this._target,this._target,e),this.onChange(),this}inverse(e=this._target){return me(this._target,e),this.onChange(),this}conjugate(e=this._target){return he(this._target,e),this.onChange(),this}copy(e){return X(this._target,e),this.onChange(),this}normalize(e=this._target){return ye(this._target,e),this.onChange(),this}multiply(e,t){return t?le(this._target,e,t):le(this._target,this._target,e),this.onChange(),this}dot(e){return ve(this._target,e)}fromMatrix3(e){return ge(this._target,e),this.onChange(),this}fromEuler(e,t){return Y(this._target,e,e.order),t||this.onChange(),this}fromAxisAngle(e,t){return ce(this._target,e,t),this.onChange(),this}slerp(e,t){return pe(this._target,this._target,e,t),this.onChange(),this}fromArray(e,t=0){return this._target[0]=e[t],this._target[1]=e[t+1],this._target[2]=e[t+2],this._target[3]=e[t+3],this.onChange(),this}toArray(e=[],t=0){return e[t]=this[0],e[t+1]=this[1],e[t+2]=this[2],e[t+3]=this[3],e}},xe=1e-6;function Se(e,t){return e[0]=t[0],e[1]=t[1],e[2]=t[2],e[3]=t[3],e[4]=t[4],e[5]=t[5],e[6]=t[6],e[7]=t[7],e[8]=t[8],e[9]=t[9],e[10]=t[10],e[11]=t[11],e[12]=t[12],e[13]=t[13],e[14]=t[14],e[15]=t[15],e}function Ce(e,t,n,r,i,a,o,s,c,l,u,d,f,p,m,h,g){return e[0]=t,e[1]=n,e[2]=r,e[3]=i,e[4]=a,e[5]=o,e[6]=s,e[7]=c,e[8]=l,e[9]=u,e[10]=d,e[11]=f,e[12]=p,e[13]=m,e[14]=h,e[15]=g,e}function we(e){return e[0]=1,e[1]=0,e[2]=0,e[3]=0,e[4]=0,e[5]=1,e[6]=0,e[7]=0,e[8]=0,e[9]=0,e[10]=1,e[11]=0,e[12]=0,e[13]=0,e[14]=0,e[15]=1,e}function Te(e,t){let n=t[0],r=t[1],i=t[2],a=t[3],o=t[4],s=t[5],c=t[6],l=t[7],u=t[8],d=t[9],f=t[10],p=t[11],m=t[12],h=t[13],g=t[14],_=t[15],v=n*s-r*o,y=n*c-i*o,b=n*l-a*o,x=r*c-i*s,S=r*l-a*s,C=i*l-a*c,w=u*h-d*m,T=u*g-f*m,E=u*_-p*m,D=d*g-f*h,O=d*_-p*h,k=f*_-p*g,A=v*k-y*O+b*D+x*E-S*T+C*w;return A?(A=1/A,e[0]=(s*k-c*O+l*D)*A,e[1]=(i*O-r*k-a*D)*A,e[2]=(h*C-g*S+_*x)*A,e[3]=(f*S-d*C-p*x)*A,e[4]=(c*E-o*k-l*T)*A,e[5]=(n*k-i*E+a*T)*A,e[6]=(g*b-m*C-_*y)*A,e[7]=(u*C-f*b+p*y)*A,e[8]=(o*O-s*E+l*w)*A,e[9]=(r*E-n*O-a*w)*A,e[10]=(m*S-h*b+_*v)*A,e[11]=(d*b-u*S-p*v)*A,e[12]=(s*T-o*D-c*w)*A,e[13]=(n*D-r*T+i*w)*A,e[14]=(h*y-m*x-g*v)*A,e[15]=(u*x-d*y+f*v)*A,e):null}function Ee(e){let t=e[0],n=e[1],r=e[2],i=e[3],a=e[4],o=e[5],s=e[6],c=e[7],l=e[8],u=e[9],d=e[10],f=e[11],p=e[12],m=e[13],h=e[14],g=e[15],_=t*o-n*a,v=t*s-r*a,y=t*c-i*a,b=n*s-r*o,x=n*c-i*o,S=r*c-i*s,C=l*m-u*p,w=l*h-d*p,T=l*g-f*p,E=u*h-d*m,D=u*g-f*m;return _*(d*g-f*h)-v*D+y*E+b*T-x*w+S*C}function Z(e,t,n){let r=t[0],i=t[1],a=t[2],o=t[3],s=t[4],c=t[5],l=t[6],u=t[7],d=t[8],f=t[9],p=t[10],m=t[11],h=t[12],g=t[13],_=t[14],v=t[15],y=n[0],b=n[1],x=n[2],S=n[3];return e[0]=y*r+b*s+x*d+S*h,e[1]=y*i+b*c+x*f+S*g,e[2]=y*a+b*l+x*p+S*_,e[3]=y*o+b*u+x*m+S*v,y=n[4],b=n[5],x=n[6],S=n[7],e[4]=y*r+b*s+x*d+S*h,e[5]=y*i+b*c+x*f+S*g,e[6]=y*a+b*l+x*p+S*_,e[7]=y*o+b*u+x*m+S*v,y=n[8],b=n[9],x=n[10],S=n[11],e[8]=y*r+b*s+x*d+S*h,e[9]=y*i+b*c+x*f+S*g,e[10]=y*a+b*l+x*p+S*_,e[11]=y*o+b*u+x*m+S*v,y=n[12],b=n[13],x=n[14],S=n[15],e[12]=y*r+b*s+x*d+S*h,e[13]=y*i+b*c+x*f+S*g,e[14]=y*a+b*l+x*p+S*_,e[15]=y*o+b*u+x*m+S*v,e}function De(e,t,n){let r=n[0],i=n[1],a=n[2],o,s,c,l,u,d,f,p,m,h,g,_;return t===e?(e[12]=t[0]*r+t[4]*i+t[8]*a+t[12],e[13]=t[1]*r+t[5]*i+t[9]*a+t[13],e[14]=t[2]*r+t[6]*i+t[10]*a+t[14],e[15]=t[3]*r+t[7]*i+t[11]*a+t[15]):(o=t[0],s=t[1],c=t[2],l=t[3],u=t[4],d=t[5],f=t[6],p=t[7],m=t[8],h=t[9],g=t[10],_=t[11],e[0]=o,e[1]=s,e[2]=c,e[3]=l,e[4]=u,e[5]=d,e[6]=f,e[7]=p,e[8]=m,e[9]=h,e[10]=g,e[11]=_,e[12]=o*r+u*i+m*a+t[12],e[13]=s*r+d*i+h*a+t[13],e[14]=c*r+f*i+g*a+t[14],e[15]=l*r+p*i+_*a+t[15]),e}function Oe(e,t,n){let r=n[0],i=n[1],a=n[2];return e[0]=t[0]*r,e[1]=t[1]*r,e[2]=t[2]*r,e[3]=t[3]*r,e[4]=t[4]*i,e[5]=t[5]*i,e[6]=t[6]*i,e[7]=t[7]*i,e[8]=t[8]*a,e[9]=t[9]*a,e[10]=t[10]*a,e[11]=t[11]*a,e[12]=t[12],e[13]=t[13],e[14]=t[14],e[15]=t[15],e}function ke(e,t,n,r){let i=r[0],a=r[1],o=r[2],s=Math.hypot(i,a,o),c,l,u,d,f,p,m,h,g,_,v,y,b,x,S,C,w,T,E,D,O,k,A,j;return Math.abs(s)<xe?null:(s=1/s,i*=s,a*=s,o*=s,c=Math.sin(n),l=Math.cos(n),u=1-l,d=t[0],f=t[1],p=t[2],m=t[3],h=t[4],g=t[5],_=t[6],v=t[7],y=t[8],b=t[9],x=t[10],S=t[11],C=i*i*u+l,w=a*i*u+o*c,T=o*i*u-a*c,E=i*a*u-o*c,D=a*a*u+l,O=o*a*u+i*c,k=i*o*u+a*c,A=a*o*u-i*c,j=o*o*u+l,e[0]=d*C+h*w+y*T,e[1]=f*C+g*w+b*T,e[2]=p*C+_*w+x*T,e[3]=m*C+v*w+S*T,e[4]=d*E+h*D+y*O,e[5]=f*E+g*D+b*O,e[6]=p*E+_*D+x*O,e[7]=m*E+v*D+S*O,e[8]=d*k+h*A+y*j,e[9]=f*k+g*A+b*j,e[10]=p*k+_*A+x*j,e[11]=m*k+v*A+S*j,t!==e&&(e[12]=t[12],e[13]=t[13],e[14]=t[14],e[15]=t[15]),e)}function Ae(e,t){return e[0]=t[12],e[1]=t[13],e[2]=t[14],e}function je(e,t){let n=t[0],r=t[1],i=t[2],a=t[4],o=t[5],s=t[6],c=t[8],l=t[9],u=t[10];return e[0]=Math.hypot(n,r,i),e[1]=Math.hypot(a,o,s),e[2]=Math.hypot(c,l,u),e}function Me(e){let t=e[0],n=e[1],r=e[2],i=e[4],a=e[5],o=e[6],s=e[8],c=e[9],l=e[10],u=t*t+n*n+r*r,d=i*i+a*a+o*o,f=s*s+c*c+l*l;return Math.sqrt(Math.max(u,d,f))}var Ne=(function(){let e=[1,1,1];return function(t,n){let r=e;je(r,n);let i=1/r[0],a=1/r[1],o=1/r[2],s=n[0]*i,c=n[1]*a,l=n[2]*o,u=n[4]*i,d=n[5]*a,f=n[6]*o,p=n[8]*i,m=n[9]*a,h=n[10]*o,g=s+d+h,_=0;return g>0?(_=Math.sqrt(g+1)*2,t[3]=.25*_,t[0]=(f-m)/_,t[1]=(p-l)/_,t[2]=(c-u)/_):s>d&&s>h?(_=Math.sqrt(1+s-d-h)*2,t[3]=(f-m)/_,t[0]=.25*_,t[1]=(c+u)/_,t[2]=(p+l)/_):d>h?(_=Math.sqrt(1+d-s-h)*2,t[3]=(p-l)/_,t[0]=(c+u)/_,t[1]=.25*_,t[2]=(f+m)/_):(_=Math.sqrt(1+h-s-d)*2,t[3]=(c-u)/_,t[0]=(p+l)/_,t[1]=(f+m)/_,t[2]=.25*_),t}})();function Pe(e,t,n,r){let i=l([e[0],e[1],e[2]]),a=l([e[4],e[5],e[6]]),o=l([e[8],e[9],e[10]]);Ee(e)<0&&(i=-i),n[0]=e[12],n[1]=e[13],n[2]=e[14];let s=e.slice(),c=1/i,u=1/a,d=1/o;s[0]*=c,s[1]*=c,s[2]*=c,s[4]*=u,s[5]*=u,s[6]*=u,s[8]*=d,s[9]*=d,s[10]*=d,Ne(t,s),r[0]=i,r[1]=a,r[2]=o}function Fe(e,t,n,r){let i=e,a=t[0],o=t[1],s=t[2],c=t[3],l=a+a,u=o+o,d=s+s,f=a*l,p=a*u,m=a*d,h=o*u,g=o*d,_=s*d,v=c*l,y=c*u,b=c*d,x=r[0],S=r[1],C=r[2];return i[0]=(1-(h+_))*x,i[1]=(p+b)*x,i[2]=(m-y)*x,i[3]=0,i[4]=(p-b)*S,i[5]=(1-(f+_))*S,i[6]=(g+v)*S,i[7]=0,i[8]=(m+y)*C,i[9]=(g-v)*C,i[10]=(1-(f+h))*C,i[11]=0,i[12]=n[0],i[13]=n[1],i[14]=n[2],i[15]=1,i}function Ie(e,t){let n=t[0],r=t[1],i=t[2],a=t[3],o=n+n,s=r+r,c=i+i,l=n*o,u=r*o,d=r*s,f=i*o,p=i*s,m=i*c,h=a*o,g=a*s,_=a*c;return e[0]=1-d-m,e[1]=u+_,e[2]=f-g,e[3]=0,e[4]=u-_,e[5]=1-l-m,e[6]=p+h,e[7]=0,e[8]=f+g,e[9]=p-h,e[10]=1-l-d,e[11]=0,e[12]=0,e[13]=0,e[14]=0,e[15]=1,e}function Le(e,t,n,r,i){let a=1/Math.tan(t/2),o=1/(r-i);return e[0]=a/n,e[1]=0,e[2]=0,e[3]=0,e[4]=0,e[5]=a,e[6]=0,e[7]=0,e[8]=0,e[9]=0,e[10]=(i+r)*o,e[11]=-1,e[12]=0,e[13]=0,e[14]=2*i*r*o,e[15]=0,e}function Re(e,t,n,r,i,a,o){let s=1/(t-n),c=1/(r-i),l=1/(a-o);return e[0]=-2*s,e[1]=0,e[2]=0,e[3]=0,e[4]=0,e[5]=-2*c,e[6]=0,e[7]=0,e[8]=0,e[9]=0,e[10]=2*l,e[11]=0,e[12]=(t+n)*s,e[13]=(i+r)*c,e[14]=(o+a)*l,e[15]=1,e}function ze(e,t,n,r){let i=t[0],a=t[1],o=t[2],s=r[0],c=r[1],l=r[2],u=i-n[0],d=a-n[1],f=o-n[2],p=u*u+d*d+f*f;p===0?f=1:(p=1/Math.sqrt(p),u*=p,d*=p,f*=p);let m=c*f-l*d,h=l*u-s*f,g=s*d-c*u;return p=m*m+h*h+g*g,p===0&&(l?s+=1e-6:c?l+=1e-6:c+=1e-6,m=c*f-l*d,h=l*u-s*f,g=s*d-c*u,p=m*m+h*h+g*g),p=1/Math.sqrt(p),m*=p,h*=p,g*=p,e[0]=m,e[1]=h,e[2]=g,e[3]=0,e[4]=d*g-f*h,e[5]=f*m-u*g,e[6]=u*h-d*m,e[7]=0,e[8]=u,e[9]=d,e[10]=f,e[11]=0,e[12]=i,e[13]=a,e[14]=o,e[15]=1,e}function Be(e,t,n){return e[0]=t[0]+n[0],e[1]=t[1]+n[1],e[2]=t[2]+n[2],e[3]=t[3]+n[3],e[4]=t[4]+n[4],e[5]=t[5]+n[5],e[6]=t[6]+n[6],e[7]=t[7]+n[7],e[8]=t[8]+n[8],e[9]=t[9]+n[9],e[10]=t[10]+n[10],e[11]=t[11]+n[11],e[12]=t[12]+n[12],e[13]=t[13]+n[13],e[14]=t[14]+n[14],e[15]=t[15]+n[15],e}function Ve(e,t,n){return e[0]=t[0]-n[0],e[1]=t[1]-n[1],e[2]=t[2]-n[2],e[3]=t[3]-n[3],e[4]=t[4]-n[4],e[5]=t[5]-n[5],e[6]=t[6]-n[6],e[7]=t[7]-n[7],e[8]=t[8]-n[8],e[9]=t[9]-n[9],e[10]=t[10]-n[10],e[11]=t[11]-n[11],e[12]=t[12]-n[12],e[13]=t[13]-n[13],e[14]=t[14]-n[14],e[15]=t[15]-n[15],e}function He(e,t,n){return e[0]=t[0]*n,e[1]=t[1]*n,e[2]=t[2]*n,e[3]=t[3]*n,e[4]=t[4]*n,e[5]=t[5]*n,e[6]=t[6]*n,e[7]=t[7]*n,e[8]=t[8]*n,e[9]=t[9]*n,e[10]=t[10]*n,e[11]=t[11]*n,e[12]=t[12]*n,e[13]=t[13]*n,e[14]=t[14]*n,e[15]=t[15]*n,e}var Ue=class extends Array{constructor(e=1,t=0,n=0,r=0,i=0,a=1,o=0,s=0,c=0,l=0,u=1,d=0,f=0,p=0,m=0,h=1){return super(e,t,n,r,i,a,o,s,c,l,u,d,f,p,m,h),this}get x(){return this[12]}get y(){return this[13]}get z(){return this[14]}get w(){return this[15]}set x(e){this[12]=e}set y(e){this[13]=e}set z(e){this[14]=e}set w(e){this[15]=e}set(e,t,n,r,i,a,o,s,c,l,u,d,f,p,m,h){return e.length?this.copy(e):(Ce(this,e,t,n,r,i,a,o,s,c,l,u,d,f,p,m,h),this)}translate(e,t=this){return De(this,t,e),this}rotate(e,t,n=this){return ke(this,n,e,t),this}scale(e,t=this){return Oe(this,t,typeof e==`number`?[e,e,e]:e),this}add(e,t){return t?Be(this,e,t):Be(this,this,e),this}sub(e,t){return t?Ve(this,e,t):Ve(this,this,e),this}multiply(e,t){return e.length?t?Z(this,e,t):Z(this,this,e):He(this,this,e),this}identity(){return we(this),this}copy(e){return Se(this,e),this}fromPerspective({fov:e,aspect:t,near:n,far:r}={}){return Le(this,e,t,n,r),this}fromOrthogonal({left:e,right:t,bottom:n,top:r,near:i,far:a}){return Re(this,e,t,n,r,i,a),this}fromQuaternion(e){return Ie(this,e),this}setPosition(e){return this.x=e[0],this.y=e[1],this.z=e[2],this}inverse(e=this){return Te(this,e),this}compose(e,t,n){return Fe(this,e,t,n),this}decompose(e,t,n){return Pe(this,e,t,n),this}getRotation(e){return Ne(e,this),this}getTranslation(e){return Ae(e,this),this}getScaling(e){return je(e,this),this}getMaxScaleOnAxis(){return Me(this)}lookAt(e,t,n){return ze(this,e,t,n),this}determinant(){return Ee(this)}fromArray(e,t=0){return this[0]=e[t],this[1]=e[t+1],this[2]=e[t+2],this[3]=e[t+3],this[4]=e[t+4],this[5]=e[t+5],this[6]=e[t+6],this[7]=e[t+7],this[8]=e[t+8],this[9]=e[t+9],this[10]=e[t+10],this[11]=e[t+11],this[12]=e[t+12],this[13]=e[t+13],this[14]=e[t+14],this[15]=e[t+15],this}toArray(e=[],t=0){return e[t]=this[0],e[t+1]=this[1],e[t+2]=this[2],e[t+3]=this[3],e[t+4]=this[4],e[t+5]=this[5],e[t+6]=this[6],e[t+7]=this[7],e[t+8]=this[8],e[t+9]=this[9],e[t+10]=this[10],e[t+11]=this[11],e[t+12]=this[12],e[t+13]=this[13],e[t+14]=this[14],e[t+15]=this[15],e}};function We(e,t,n=`YXZ`){return n===`XYZ`?(e[1]=Math.asin(Math.min(Math.max(t[8],-1),1)),Math.abs(t[8])<.99999?(e[0]=Math.atan2(-t[9],t[10]),e[2]=Math.atan2(-t[4],t[0])):(e[0]=Math.atan2(t[6],t[5]),e[2]=0)):n===`YXZ`?(e[0]=Math.asin(-Math.min(Math.max(t[9],-1),1)),Math.abs(t[9])<.99999?(e[1]=Math.atan2(t[8],t[10]),e[2]=Math.atan2(t[1],t[5])):(e[1]=Math.atan2(-t[2],t[0]),e[2]=0)):n===`ZXY`?(e[0]=Math.asin(Math.min(Math.max(t[6],-1),1)),Math.abs(t[6])<.99999?(e[1]=Math.atan2(-t[2],t[10]),e[2]=Math.atan2(-t[4],t[5])):(e[1]=0,e[2]=Math.atan2(t[1],t[0]))):n===`ZYX`?(e[1]=Math.asin(-Math.min(Math.max(t[2],-1),1)),Math.abs(t[2])<.99999?(e[0]=Math.atan2(t[6],t[10]),e[2]=Math.atan2(t[1],t[0])):(e[0]=0,e[2]=Math.atan2(-t[4],t[5]))):n===`YZX`?(e[2]=Math.asin(Math.min(Math.max(t[1],-1),1)),Math.abs(t[1])<.99999?(e[0]=Math.atan2(-t[9],t[5]),e[1]=Math.atan2(-t[2],t[0])):(e[0]=0,e[1]=Math.atan2(t[8],t[10]))):n===`XZY`&&(e[2]=Math.asin(-Math.min(Math.max(t[4],-1),1)),Math.abs(t[4])<.99999?(e[0]=Math.atan2(t[6],t[5]),e[1]=Math.atan2(t[8],t[0])):(e[0]=Math.atan2(-t[9],t[10]),e[1]=0)),e}var Ge=new Ue,Ke=class extends Array{constructor(e=0,t=e,n=e,r=`YXZ`){super(e,t,n),this.order=r,this.onChange=()=>{},this._target=this;let i=[`0`,`1`,`2`];return new Proxy(this,{set(e,t){let n=Reflect.set(...arguments);return n&&i.includes(t)&&e.onChange(),n}})}get x(){return this[0]}get y(){return this[1]}get z(){return this[2]}set x(e){this._target[0]=e,this.onChange()}set y(e){this._target[1]=e,this.onChange()}set z(e){this._target[2]=e,this.onChange()}set(e,t=e,n=e){return e.length?this.copy(e):(this._target[0]=e,this._target[1]=t,this._target[2]=n,this.onChange(),this)}copy(e){return this._target[0]=e[0],this._target[1]=e[1],this._target[2]=e[2],this.onChange(),this}reorder(e){return this._target.order=e,this.onChange(),this}fromRotationMatrix(e,t=this.order){return We(this._target,e,t),this.onChange(),this}fromQuaternion(e,t=this.order,n){return Ge.fromQuaternion(e),this._target.fromRotationMatrix(Ge,t),n||this.onChange(),this}fromArray(e,t=0){return this._target[0]=e[t],this._target[1]=e[t+1],this._target[2]=e[t+2],this}toArray(e=[],t=0){return e[t]=this[0],e[t+1]=this[1],e[t+2]=this[2],e}},qe=class{constructor(){this.parent=null,this.children=[],this.visible=!0,this.matrix=new Ue,this.worldMatrix=new Ue,this.matrixAutoUpdate=!0,this.worldMatrixNeedsUpdate=!1,this.position=new N,this.quaternion=new be,this.scale=new N(1),this.rotation=new Ke,this.up=new N(0,1,0),this.rotation._target.onChange=()=>this.quaternion.fromEuler(this.rotation,!0),this.quaternion._target.onChange=()=>this.rotation.fromQuaternion(this.quaternion,void 0,!0)}setParent(e,t=!0){this.parent&&e!==this.parent&&this.parent.removeChild(this,!1),this.parent=e,t&&e&&e.addChild(this,!1)}addChild(e,t=!0){~this.children.indexOf(e)||this.children.push(e),t&&e.setParent(this,!1)}removeChild(e,t=!0){~this.children.indexOf(e)&&this.children.splice(this.children.indexOf(e),1),t&&e.setParent(null,!1)}updateMatrixWorld(e){this.matrixAutoUpdate&&this.updateMatrix(),(this.worldMatrixNeedsUpdate||e)&&(this.parent===null?this.worldMatrix.copy(this.matrix):this.worldMatrix.multiply(this.parent.worldMatrix,this.matrix),this.worldMatrixNeedsUpdate=!1,e=!0);for(let t=0,n=this.children.length;t<n;t++)this.children[t].updateMatrixWorld(e)}updateMatrix(){this.matrix.compose(this.quaternion,this.position,this.scale),this.worldMatrixNeedsUpdate=!0}traverse(e){if(!e(this))for(let t=0,n=this.children.length;t<n;t++)this.children[t].traverse(e)}decompose(){this.matrix.decompose(this.quaternion._target,this.position,this.scale),this.rotation.fromQuaternion(this.quaternion)}lookAt(e,t=!1){t?this.matrix.lookAt(this.position,e,this.up):this.matrix.lookAt(e,this.position,this.up),this.matrix.getRotation(this.quaternion._target),this.rotation.fromQuaternion(this.quaternion)}};function Je(e,t){return e[0]=t[0],e[1]=t[1],e[2]=t[2],e[3]=t[4],e[4]=t[5],e[5]=t[6],e[6]=t[8],e[7]=t[9],e[8]=t[10],e}function Ye(e,t){let n=t[0],r=t[1],i=t[2],a=t[3],o=n+n,s=r+r,c=i+i,l=n*o,u=r*o,d=r*s,f=i*o,p=i*s,m=i*c,h=a*o,g=a*s,_=a*c;return e[0]=1-d-m,e[3]=u-_,e[6]=f+g,e[1]=u+_,e[4]=1-l-m,e[7]=p-h,e[2]=f-g,e[5]=p+h,e[8]=1-l-d,e}function Xe(e,t){return e[0]=t[0],e[1]=t[1],e[2]=t[2],e[3]=t[3],e[4]=t[4],e[5]=t[5],e[6]=t[6],e[7]=t[7],e[8]=t[8],e}function Ze(e,t,n,r,i,a,o,s,c,l){return e[0]=t,e[1]=n,e[2]=r,e[3]=i,e[4]=a,e[5]=o,e[6]=s,e[7]=c,e[8]=l,e}function Qe(e){return e[0]=1,e[1]=0,e[2]=0,e[3]=0,e[4]=1,e[5]=0,e[6]=0,e[7]=0,e[8]=1,e}function $e(e,t){let n=t[0],r=t[1],i=t[2],a=t[3],o=t[4],s=t[5],c=t[6],l=t[7],u=t[8],d=u*o-s*l,f=-u*a+s*c,p=l*a-o*c,m=n*d+r*f+i*p;return m?(m=1/m,e[0]=d*m,e[1]=(-u*r+i*l)*m,e[2]=(s*r-i*o)*m,e[3]=f*m,e[4]=(u*n-i*c)*m,e[5]=(-s*n+i*a)*m,e[6]=p*m,e[7]=(-l*n+r*c)*m,e[8]=(o*n-r*a)*m,e):null}function et(e,t,n){let r=t[0],i=t[1],a=t[2],o=t[3],s=t[4],c=t[5],l=t[6],u=t[7],d=t[8],f=n[0],p=n[1],m=n[2],h=n[3],g=n[4],_=n[5],v=n[6],y=n[7],b=n[8];return e[0]=f*r+p*o+m*l,e[1]=f*i+p*s+m*u,e[2]=f*a+p*c+m*d,e[3]=h*r+g*o+_*l,e[4]=h*i+g*s+_*u,e[5]=h*a+g*c+_*d,e[6]=v*r+y*o+b*l,e[7]=v*i+y*s+b*u,e[8]=v*a+y*c+b*d,e}function tt(e,t,n){let r=t[0],i=t[1],a=t[2],o=t[3],s=t[4],c=t[5],l=t[6],u=t[7],d=t[8],f=n[0],p=n[1];return e[0]=r,e[1]=i,e[2]=a,e[3]=o,e[4]=s,e[5]=c,e[6]=f*r+p*o+l,e[7]=f*i+p*s+u,e[8]=f*a+p*c+d,e}function nt(e,t,n){let r=t[0],i=t[1],a=t[2],o=t[3],s=t[4],c=t[5],l=t[6],u=t[7],d=t[8],f=Math.sin(n),p=Math.cos(n);return e[0]=p*r+f*o,e[1]=p*i+f*s,e[2]=p*a+f*c,e[3]=p*o-f*r,e[4]=p*s-f*i,e[5]=p*c-f*a,e[6]=l,e[7]=u,e[8]=d,e}function rt(e,t,n){let r=n[0],i=n[1];return e[0]=r*t[0],e[1]=r*t[1],e[2]=r*t[2],e[3]=i*t[3],e[4]=i*t[4],e[5]=i*t[5],e[6]=t[6],e[7]=t[7],e[8]=t[8],e}function it(e,t){let n=t[0],r=t[1],i=t[2],a=t[3],o=t[4],s=t[5],c=t[6],l=t[7],u=t[8],d=t[9],f=t[10],p=t[11],m=t[12],h=t[13],g=t[14],_=t[15],v=n*s-r*o,y=n*c-i*o,b=n*l-a*o,x=r*c-i*s,S=r*l-a*s,C=i*l-a*c,w=u*h-d*m,T=u*g-f*m,E=u*_-p*m,D=d*g-f*h,O=d*_-p*h,k=f*_-p*g,A=v*k-y*O+b*D+x*E-S*T+C*w;return A?(A=1/A,e[0]=(s*k-c*O+l*D)*A,e[1]=(c*E-o*k-l*T)*A,e[2]=(o*O-s*E+l*w)*A,e[3]=(i*O-r*k-a*D)*A,e[4]=(n*k-i*E+a*T)*A,e[5]=(r*E-n*O-a*w)*A,e[6]=(h*C-g*S+_*x)*A,e[7]=(g*b-m*C-_*y)*A,e[8]=(m*S-h*b+_*v)*A,e):null}var at=class extends Array{constructor(e=1,t=0,n=0,r=0,i=1,a=0,o=0,s=0,c=1){return super(e,t,n,r,i,a,o,s,c),this}set(e,t,n,r,i,a,o,s,c){return e.length?this.copy(e):(Ze(this,e,t,n,r,i,a,o,s,c),this)}translate(e,t=this){return tt(this,t,e),this}rotate(e,t=this){return nt(this,t,e),this}scale(e,t=this){return rt(this,t,e),this}multiply(e,t){return t?et(this,e,t):et(this,this,e),this}identity(){return Qe(this),this}copy(e){return Xe(this,e),this}fromMatrix4(e){return Je(this,e),this}fromQuaternion(e){return Ye(this,e),this}fromBasis(e,t,n){return this.set(e[0],e[1],e[2],t[0],t[1],t[2],n[0],n[1],n[2]),this}inverse(e=this){return $e(this,e),this}getNormalMatrix(e){return it(this,e),this}},ot=0,st=class extends qe{constructor(e,{geometry:t,program:n,mode:r=e.TRIANGLES,frustumCulled:i=!0,renderOrder:a=0}={}){super(),e.canvas||console.error(`gl not passed as first argument to Mesh`),this.gl=e,this.id=ot++,this.geometry=t,this.program=n,this.mode=r,this.frustumCulled=i,this.renderOrder=a,this.modelViewMatrix=new Ue,this.normalMatrix=new at,this.beforeRenderCallbacks=[],this.afterRenderCallbacks=[]}onBeforeRender(e){return this.beforeRenderCallbacks.push(e),this}onAfterRender(e){return this.afterRenderCallbacks.push(e),this}draw({camera:e}={}){e&&(this.program.uniforms.modelMatrix||Object.assign(this.program.uniforms,{modelMatrix:{value:null},viewMatrix:{value:null},modelViewMatrix:{value:null},normalMatrix:{value:null},projectionMatrix:{value:null},cameraPosition:{value:null}}),this.program.uniforms.projectionMatrix.value=e.projectionMatrix,this.program.uniforms.cameraPosition.value=e.worldPosition,this.program.uniforms.viewMatrix.value=e.viewMatrix,this.modelViewMatrix.multiply(e.viewMatrix,this.worldMatrix),this.normalMatrix.getNormalMatrix(this.modelViewMatrix),this.program.uniforms.modelMatrix.value=this.worldMatrix,this.program.uniforms.modelViewMatrix.value=this.modelViewMatrix,this.program.uniforms.normalMatrix.value=this.normalMatrix),this.beforeRenderCallbacks.forEach(t=>t&&t({mesh:this,camera:e}));let t=this.program.cullFace&&this.worldMatrix.determinant()<0;this.program.use({flipFaces:t}),this.geometry.draw({mode:this.mode,program:this.program}),this.afterRenderCallbacks.forEach(t=>t&&t({mesh:this,camera:e}))}},ct=new Uint8Array(4);function lt(e){return!(e&e-1)}var ut=1,dt=class{constructor(e,{image:t,target:n=e.TEXTURE_2D,type:r=e.UNSIGNED_BYTE,format:i=e.RGBA,internalFormat:a=i,wrapS:o=e.CLAMP_TO_EDGE,wrapT:s=e.CLAMP_TO_EDGE,wrapR:c=e.CLAMP_TO_EDGE,generateMipmaps:l=n===(e.TEXTURE_2D||e.TEXTURE_CUBE_MAP),minFilter:u=l?e.NEAREST_MIPMAP_LINEAR:e.LINEAR,magFilter:d=e.LINEAR,premultiplyAlpha:f=!1,unpackAlignment:p=4,flipY:m=n==(e.TEXTURE_2D||e.TEXTURE_3D),anisotropy:h=0,level:g=0,width:_,height:v=_,length:y=1}={}){this.gl=e,this.id=ut++,this.image=t,this.target=n,this.type=r,this.format=i,this.internalFormat=a,this.minFilter=u,this.magFilter=d,this.wrapS=o,this.wrapT=s,this.wrapR=c,this.generateMipmaps=l,this.premultiplyAlpha=f,this.unpackAlignment=p,this.flipY=m,this.anisotropy=Math.min(h,this.gl.renderer.parameters.maxAnisotropy),this.level=g,this.width=_,this.height=v,this.length=y,this.texture=this.gl.createTexture(),this.store={image:null},this.glState=this.gl.renderer.state,this.state={},this.state.minFilter=this.gl.NEAREST_MIPMAP_LINEAR,this.state.magFilter=this.gl.LINEAR,this.state.wrapS=this.gl.REPEAT,this.state.wrapT=this.gl.REPEAT,this.state.anisotropy=0}bind(){this.glState.textureUnits[this.glState.activeTextureUnit]!==this.id&&(this.gl.bindTexture(this.target,this.texture),this.glState.textureUnits[this.glState.activeTextureUnit]=this.id)}update(e=0){let t=!(this.image===this.store.image&&!this.needsUpdate);if((t||this.glState.textureUnits[e]!==this.id)&&(this.gl.renderer.activeTexture(e),this.bind()),t){if(this.needsUpdate=!1,this.flipY!==this.glState.flipY&&(this.gl.pixelStorei(this.gl.UNPACK_FLIP_Y_WEBGL,this.flipY),this.glState.flipY=this.flipY),this.premultiplyAlpha!==this.glState.premultiplyAlpha&&(this.gl.pixelStorei(this.gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL,this.premultiplyAlpha),this.glState.premultiplyAlpha=this.premultiplyAlpha),this.unpackAlignment!==this.glState.unpackAlignment&&(this.gl.pixelStorei(this.gl.UNPACK_ALIGNMENT,this.unpackAlignment),this.glState.unpackAlignment=this.unpackAlignment),this.minFilter!==this.state.minFilter&&(this.gl.texParameteri(this.target,this.gl.TEXTURE_MIN_FILTER,this.minFilter),this.state.minFilter=this.minFilter),this.magFilter!==this.state.magFilter&&(this.gl.texParameteri(this.target,this.gl.TEXTURE_MAG_FILTER,this.magFilter),this.state.magFilter=this.magFilter),this.wrapS!==this.state.wrapS&&(this.gl.texParameteri(this.target,this.gl.TEXTURE_WRAP_S,this.wrapS),this.state.wrapS=this.wrapS),this.wrapT!==this.state.wrapT&&(this.gl.texParameteri(this.target,this.gl.TEXTURE_WRAP_T,this.wrapT),this.state.wrapT=this.wrapT),this.wrapR!==this.state.wrapR&&(this.gl.texParameteri(this.target,this.gl.TEXTURE_WRAP_R,this.wrapR),this.state.wrapR=this.wrapR),this.anisotropy&&this.anisotropy!==this.state.anisotropy&&(this.gl.texParameterf(this.target,this.gl.renderer.getExtension(`EXT_texture_filter_anisotropic`).TEXTURE_MAX_ANISOTROPY_EXT,this.anisotropy),this.state.anisotropy=this.anisotropy),this.image){if(this.image.width&&(this.width=this.image.width,this.height=this.image.height),this.target===this.gl.TEXTURE_CUBE_MAP)for(let e=0;e<6;e++)this.gl.texImage2D(this.gl.TEXTURE_CUBE_MAP_POSITIVE_X+e,this.level,this.internalFormat,this.format,this.type,this.image[e]);else if(ArrayBuffer.isView(this.image))this.target===this.gl.TEXTURE_2D?this.gl.texImage2D(this.target,this.level,this.internalFormat,this.width,this.height,0,this.format,this.type,this.image):(this.target===this.gl.TEXTURE_2D_ARRAY||this.target===this.gl.TEXTURE_3D)&&this.gl.texImage3D(this.target,this.level,this.internalFormat,this.width,this.height,this.length,0,this.format,this.type,this.image);else if(this.image.isCompressedTexture)for(let e=0;e<this.image.length;e++)this.gl.compressedTexImage2D(this.target,e,this.internalFormat,this.image[e].width,this.image[e].height,0,this.image[e].data);else this.target===this.gl.TEXTURE_2D?this.gl.texImage2D(this.target,this.level,this.internalFormat,this.format,this.type,this.image):this.gl.texImage3D(this.target,this.level,this.internalFormat,this.width,this.height,this.length,0,this.format,this.type,this.image);this.generateMipmaps&&(!this.gl.renderer.isWebgl2&&(!lt(this.image.width)||!lt(this.image.height))?(this.generateMipmaps=!1,this.wrapS=this.wrapT=this.gl.CLAMP_TO_EDGE,this.minFilter=this.gl.LINEAR):this.gl.generateMipmap(this.target)),this.onUpdate&&this.onUpdate()}else if(this.target===this.gl.TEXTURE_CUBE_MAP)for(let e=0;e<6;e++)this.gl.texImage2D(this.gl.TEXTURE_CUBE_MAP_POSITIVE_X+e,0,this.gl.RGBA,1,1,0,this.gl.RGBA,this.gl.UNSIGNED_BYTE,ct);else this.width?this.target===this.gl.TEXTURE_2D?this.gl.texImage2D(this.target,this.level,this.internalFormat,this.width,this.height,0,this.format,this.type,null):this.gl.texImage3D(this.target,this.level,this.internalFormat,this.width,this.height,this.length,0,this.format,this.type,null):this.gl.texImage2D(this.target,0,this.gl.RGBA,1,1,0,this.gl.RGBA,this.gl.UNSIGNED_BYTE,ct);this.store.image=this.image}}},ft=class{constructor(e,{width:t=e.canvas.width,height:n=e.canvas.height,target:r=e.FRAMEBUFFER,color:i=1,depth:a=!0,stencil:o=!1,depthTexture:s=!1,wrapS:c=e.CLAMP_TO_EDGE,wrapT:l=e.CLAMP_TO_EDGE,wrapR:u=e.CLAMP_TO_EDGE,minFilter:d=e.LINEAR,magFilter:f=d,type:p=e.UNSIGNED_BYTE,format:m=e.RGBA,internalFormat:h=m,unpackAlignment:g,premultiplyAlpha:_}={}){this.gl=e,this.width=t,this.height=n,this.depth=a,this.stencil=o,this.buffer=this.gl.createFramebuffer(),this.target=r,this.gl.renderer.bindFramebuffer(this),this.textures=[];let v=[];for(let r=0;r<i;r++)this.textures.push(new dt(e,{width:t,height:n,wrapS:c,wrapT:l,wrapR:u,minFilter:d,magFilter:f,type:p,format:m,internalFormat:h,unpackAlignment:g,premultiplyAlpha:_,flipY:!1,generateMipmaps:!1})),this.textures[r].update(),this.gl.framebufferTexture2D(this.target,this.gl.COLOR_ATTACHMENT0+r,this.gl.TEXTURE_2D,this.textures[r].texture,0),v.push(this.gl.COLOR_ATTACHMENT0+r);v.length>1&&this.gl.renderer.drawBuffers(v),this.texture=this.textures[0],s&&(this.gl.renderer.isWebgl2||this.gl.renderer.getExtension(`WEBGL_depth_texture`))?(this.depthTexture=new dt(e,{width:t,height:n,minFilter:this.gl.NEAREST,magFilter:this.gl.NEAREST,format:this.stencil?this.gl.DEPTH_STENCIL:this.gl.DEPTH_COMPONENT,internalFormat:e.renderer.isWebgl2?this.stencil?this.gl.DEPTH24_STENCIL8:this.gl.DEPTH_COMPONENT16:this.gl.DEPTH_COMPONENT,type:this.stencil?this.gl.UNSIGNED_INT_24_8:this.gl.UNSIGNED_INT}),this.depthTexture.update(),this.gl.framebufferTexture2D(this.target,this.stencil?this.gl.DEPTH_STENCIL_ATTACHMENT:this.gl.DEPTH_ATTACHMENT,this.gl.TEXTURE_2D,this.depthTexture.texture,0)):(a&&!o&&(this.depthBuffer=this.gl.createRenderbuffer(),this.gl.bindRenderbuffer(this.gl.RENDERBUFFER,this.depthBuffer),this.gl.renderbufferStorage(this.gl.RENDERBUFFER,this.gl.DEPTH_COMPONENT16,t,n),this.gl.framebufferRenderbuffer(this.target,this.gl.DEPTH_ATTACHMENT,this.gl.RENDERBUFFER,this.depthBuffer)),o&&!a&&(this.stencilBuffer=this.gl.createRenderbuffer(),this.gl.bindRenderbuffer(this.gl.RENDERBUFFER,this.stencilBuffer),this.gl.renderbufferStorage(this.gl.RENDERBUFFER,this.gl.STENCIL_INDEX8,t,n),this.gl.framebufferRenderbuffer(this.target,this.gl.STENCIL_ATTACHMENT,this.gl.RENDERBUFFER,this.stencilBuffer)),a&&o&&(this.depthStencilBuffer=this.gl.createRenderbuffer(),this.gl.bindRenderbuffer(this.gl.RENDERBUFFER,this.depthStencilBuffer),this.gl.renderbufferStorage(this.gl.RENDERBUFFER,this.gl.DEPTH_STENCIL,t,n),this.gl.framebufferRenderbuffer(this.target,this.gl.DEPTH_STENCIL_ATTACHMENT,this.gl.RENDERBUFFER,this.depthStencilBuffer))),this.gl.renderer.bindFramebuffer({target:this.target})}setSize(e,t){if(this.width!==e||this.height!==t){this.width=e,this.height=t,this.gl.renderer.bindFramebuffer(this);for(let n=0;n<this.textures.length;n++)this.textures[n].width=e,this.textures[n].height=t,this.textures[n].needsUpdate=!0,this.textures[n].update(),this.gl.framebufferTexture2D(this.target,this.gl.COLOR_ATTACHMENT0+n,this.gl.TEXTURE_2D,this.textures[n].texture,0);this.depthTexture?(this.depthTexture.width=e,this.depthTexture.height=t,this.depthTexture.needsUpdate=!0,this.depthTexture.update(),this.gl.framebufferTexture2D(this.target,this.gl.DEPTH_ATTACHMENT,this.gl.TEXTURE_2D,this.depthTexture.texture,0)):(this.depthBuffer&&(this.gl.bindRenderbuffer(this.gl.RENDERBUFFER,this.depthBuffer),this.gl.renderbufferStorage(this.gl.RENDERBUFFER,this.gl.DEPTH_COMPONENT16,e,t)),this.stencilBuffer&&(this.gl.bindRenderbuffer(this.gl.RENDERBUFFER,this.stencilBuffer),this.gl.renderbufferStorage(this.gl.RENDERBUFFER,this.gl.STENCIL_INDEX8,e,t)),this.depthStencilBuffer&&(this.gl.bindRenderbuffer(this.gl.RENDERBUFFER,this.depthStencilBuffer),this.gl.renderbufferStorage(this.gl.RENDERBUFFER,this.gl.DEPTH_STENCIL,e,t))),this.gl.renderer.bindFramebuffer({target:this.target})}}},pt=class extends ee{constructor(e,{attributes:t={}}={}){Object.assign(t,{position:{size:2,data:new Float32Array([-1,-1,3,-1,-1,3])},uv:{size:2,data:new Float32Array([0,0,2,0,0,2])}}),super(e,t)}},mt={ax:.5,ay:.3,dir:.7,width:1.3,frost:.04},Q={depth:.3,chamfer:.02,sheet:.14,lift:.3,bob:.012,podium:.95,podiumThick:.1,distance:7,yaw:0,pitch:.2,swingYaw:.16,swingPitch:.06,swayYaw:.035,swayPitch:.012};function ht(e,t,n,r,i,a,o){let s=Math.cos(r),c=[Math.sin(n)*s,Math.sin(r),Math.cos(n)*s],l=Math.hypot(c[2],c[0])||1,u=[c[2]/l,0,-c[0]/l],d=[c[1]*u[2]-c[2]*u[1],c[2]*u[0]-c[0]*u[2],c[0]*u[1]-c[1]*u[0]];return{pos:[e[0]+c[0]*t,e[1]+c[1]*t,e[2]+c[2]*t],x:u,y:d,z:c,cx:i,cy:a,f:o}}var gt=`
uniform sampler2D tLight;
uniform float uLightAlpha;
uniform vec4 uStage;      // the stage's box on the page (css px): left, top, width, height
uniform vec4 uStageInfo;  // its soft edge (px), the scene's alpha, —, the glass's alpha
uniform vec3 uCamPos;     // the camera (world); it looks along −uCamZ
uniform vec3 uCamX;
uniform vec3 uCamY;
uniform vec3 uCamZ;
uniform vec3 uCam;        // its principal point (css px), focal length (px)
uniform vec4 uMap;        // the light map's rect (world xy: left, bottom, width, height)
uniform vec4 uBeam;       // a point on the beam's axis (world xy), its direction (unit)
uniform vec4 uBeam2;      // its width, its brightness, the sheet's half thickness, the frosted cone's spread (tan)
uniform vec4 uGlassZ;     // the glass's half depth, its bevel, the light map's scale, its height above the podium
uniform vec4 uSlab;       // the podium: half width, back z, front z, thickness (a disc: radius, −radius, radius)
uniform sampler2D tSdf;   // the glass's outline: r signed distance, g distance to a fold, ba the nearest face's normal
uniform vec4 uSdfBox;     // its rect (world: left, top, width, height)
uniform vec4 uSdf;        // the distance range (±), the folds' range, the grooves' half width and depth

// the glass's outline at xy (world): signed distance (< 0 inside), distance to a fold, the nearest face's normal
vec4 sdfAt(vec2 xy) {
  vec2 uv = vec2((xy.x - uSdfBox.x) / uSdfBox.z, (uSdfBox.y - xy.y) / uSdfBox.w);
  vec4 s = texture2D(tSdf, clamp(uv, 0.0, 1.0));
  vec2 o = max(max(-uv, uv - 1.0), 0.0) * uSdfBox.zw;
  return vec4((s.r * 2.0 - 1.0) * uSdf.x + length(o), s.g * uSdf.y, s.ba * 2.0 - 1.0);
}

vec3 viewRay(vec2 p) {
  return normalize(uCamX * ((p.x - uCam.x) / uCam.z) - uCamY * ((p.y - uCam.y) / uCam.z) - uCamZ);
}
// the stage's box (px; < 0 inside)
float stageSd(vec2 p) {
  float r = uStageInfo.x;
  vec2 q = abs(p - (uStage.xy + uStage.zw * 0.5)) - uStage.zw * 0.5 + r;
  return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}
// the air of the stage: a soft pool round the glass (1 near it → 0 well before the box's edges and corners)
float stagePool(vec2 p) {
  vec2 r = (p - uCam.xy) / max(uStage.zw * 0.5, vec2(1.0));
  return (1.0 - smoothstep(-uStageInfo.x, 0.0, stageSd(p))) * (1.0 - smoothstep(0.6, 1.06, length(r)));
}
// the light at a point of the scene (world): linear rgb, the white beam ≈ 1. The beam and its trace are in the
// glass's frame (it hovers uGlassZ.w above the podium; where it meets the glass is all that matters)
vec3 lightAt3(vec3 q) {
  vec2 g = vec2(q.x, q.y - uGlassZ.w);
  // the trace covers the whole beam (the rays that miss the glass as well); the sheet is no thicker than the
  // glass, so all of it that meets the glass goes through it — away from the glass the frosted cone widens
  // the sheet (and dims it as much)
  vec3 L = texture2D(tLight, (g - uMap.xy) / uMap.zw).rgb * uGlassZ.z;
  float spread = max(0.0, sdfAt(g).x) * uBeam2.w;
  float w = uBeam2.z + spread;
  float soft = 0.32 * uBeam2.z + spread;
  L *= (1.0 - smoothstep(w - soft, w + soft, abs(q.z))) * uBeam2.z / w;
  // under the podium: the podium took it
  if (q.y < 0.0 && q.z > uSlab.y && q.z < uSlab.z && abs(q.x - q.y * uBeam.z / uBeam.w) < uSlab.x) L = vec3(0.0);
  return L * uLightAlpha;
}
`,_t=`
// Ashima / Stefan Gustavson 3D simplex noise (MIT)
vec3 mod289(vec3 x) { return x - floor(x * (1.0 / 289.0)) * 289.0; }
vec4 mod289(vec4 x) { return x - floor(x * (1.0 / 289.0)) * 289.0; }
vec4 permute(vec4 x) { return mod289(((x * 34.0) + 1.0) * x); }
vec4 taylorInvSqrt(vec4 r) { return 1.79284291400159 - 0.85373472095314 * r; }
float snoise(vec3 v) {
  const vec2 C = vec2(1.0 / 6.0, 1.0 / 3.0);
  const vec4 D = vec4(0.0, 0.5, 1.0, 2.0);
  vec3 i = floor(v + dot(v, C.yyy));
  vec3 x0 = v - i + dot(i, C.xxx);
  vec3 g = step(x0.yzx, x0.xyz);
  vec3 l = 1.0 - g;
  vec3 i1 = min(g.xyz, l.zxy);
  vec3 i2 = max(g.xyz, l.zxy);
  vec3 x1 = x0 - i1 + C.xxx;
  vec3 x2 = x0 - i2 + C.yyy;
  vec3 x3 = x0 - D.yyy;
  i = mod289(i);
  vec4 p = permute(permute(permute(i.z + vec4(0.0, i1.z, i2.z, 1.0)) + i.y + vec4(0.0, i1.y, i2.y, 1.0)) + i.x + vec4(0.0, i1.x, i2.x, 1.0));
  float n_ = 0.142857142857;
  vec3 ns = n_ * D.wyz - D.xzx;
  vec4 j = p - 49.0 * floor(p * ns.z * ns.z);
  vec4 x_ = floor(j * ns.z);
  vec4 y_ = floor(j - 7.0 * x_);
  vec4 x = x_ * ns.x + ns.yyyy;
  vec4 y = y_ * ns.x + ns.yyyy;
  vec4 h = 1.0 - abs(x) - abs(y);
  vec4 b0 = vec4(x.xy, y.xy);
  vec4 b1 = vec4(x.zw, y.zw);
  vec4 s0 = floor(b0) * 2.0 + 1.0;
  vec4 s1 = floor(b1) * 2.0 + 1.0;
  vec4 sh = -step(h, vec4(0.0));
  vec4 a0 = b0.xzyw + s0.xzyw * sh.xxyy;
  vec4 a1 = b1.xzyw + s1.xzyw * sh.zzww;
  vec3 p0 = vec3(a0.xy, h.x);
  vec3 p1 = vec3(a0.zw, h.y);
  vec3 p2 = vec3(a1.xy, h.z);
  vec3 p3 = vec3(a1.zw, h.w);
  vec4 norm = taylorInvSqrt(vec4(dot(p0, p0), dot(p1, p1), dot(p2, p2), dot(p3, p3)));
  p0 *= norm.x; p1 *= norm.y; p2 *= norm.z; p3 *= norm.w;
  vec4 m = max(0.6 - vec4(dot(x0, x0), dot(x1, x1), dot(x2, x2), dot(x3, x3)), 0.0);
  m = m * m;
  return 42.0 * dot(m * m, vec4(dot(p0, x0), dot(p1, x1), dot(p2, x2), dot(p3, x3)));
}
// divergence-free 2D flow: curl of a scalar noise field (central differences)
vec2 curl(vec2 p, float t) {
  const float e = 0.35;
  float n1 = snoise(vec3(p.x, p.y + e, t));
  float n2 = snoise(vec3(p.x, p.y - e, t));
  float n3 = snoise(vec3(p.x + e, p.y, t));
  float n4 = snoise(vec3(p.x - e, p.y, t));
  return vec2(n1 - n2, n4 - n3) / (2.0 * e);
}
`,vt=`
attribute vec2 aStart;
attribute vec2 aTarget;
// x: mode from (1 = shape, 0 = dust), y: mode to, z: delay (s), w: duration multiplier
attribute vec4 aMorph;
// x: arc sign/amount, y: scroll lag, z: dust parallax depth, w: phase
attribute vec4 aSeed;
attribute vec4 aColor;
attribute float aSize;

uniform float uDpr;
uniform vec2 uOffset;
uniform float uScrollDelta;
uniform float uTime;
uniform float uDur;
uniform float uClock;
uniform float uSpread;
uniform float uDustAmp;
uniform float uDustAlpha;
uniform float uShapeAlpha;
uniform float uShapeSize;
uniform float uTurb;
uniform float uScrollVel;
uniform vec3 uPointer;   // x, y, strength 0..1
uniform float uPointerR;
uniform vec4 uWave;      // x, y, radius (px), strength 0..1
uniform float uAlpha;
uniform float uMotion;   // 0 = static frame (reduced motion)

varying vec4 vColor;
varying float vSize;

float easeInOut(float t) {
  // smooth, magnetic: cubic in-out with a soft landing
  return t < 0.5 ? 4.0 * t * t * t : 1.0 - pow(-2.0 * t + 2.0, 3.0) * 0.5;
}

void main() {
  float raw = clamp((uTime - aMorph.z) / max(0.001, uDur * aMorph.w), 0.0, 1.0);
  float e = easeInOut(raw);
  float flight = sin(3.14159265 * e) * uMotion;   // 0 at rest, 1 mid-flight

  vec2 d = aTarget - aStart;
  vec2 base = mix(aStart, aTarget, e);
  float len = length(d);
  // a gentle arc mid-flight (perpendicular), direction per particle
  if (len > 0.5) base += vec2(-d.y, d.x) / len * flight * min(len, 520.0) * 0.16 * aSeed.x;

  float mode = aMorph.y;  // the frame of the TARGET (start was converted into it)
  vec2 pos;
  if (mode > 0.5) {
    pos = base + uOffset;
  } else {
    // dust: viewport-fixed, drifting slower than the page (parallax), wrapped vertically. The wrap
    // seam lies outside the edge fade below, so a particle crossing it is invisible at both ends.
    float m = 48.0;
    float h = uRes.y + 2.0 * m;
    float y = base.y - uScrollDelta * aSeed.z * uMotion;
    pos = vec2(base.x, mod(y + m, h) - m);
  }

  // curl-noise drift (shape: tight, dust: loose), boosted mid-flight and by scroll speed
  float shapeMix = mix(aMorph.x, aMorph.y, e);
  float amp = mix(uDustAmp, uSpread, shapeMix);
  amp *= 1.0 + uTurb * 1.6;
  amp += flight * 22.0;
  vec2 q = (mode > 0.5 ? aTarget : base) * 0.0042 + aSeed.w * 7.0;
  pos += curl(q, uClock * 0.07 + aSeed.w) * amp * 0.5 * uMotion;

  // scroll inertia: particles trail behind fast scrolling, then settle
  pos.y += clamp(uScrollVel * 0.016, -90.0, 90.0) * aSeed.y * uMotion;

  // pointer repulsion (soft gaussian push with a little swirl)
  vec2 pd = pos - uPointer.xy;
  float pl = length(pd) + 0.0001;
  float push = uPointer.z * exp(-(pl * pl) / (uPointerR * uPointerR));
  vec2 dir = pd / pl;
  pos += (dir + vec2(-dir.y, dir.x) * (aSeed.x * 0.35)) * push * uPointerR * 0.42 * uMotion;

  // shockwave ring
  if (uWave.w > 0.0) {
    vec2 wd = pos - uWave.xy;
    float wl = length(wd) + 0.0001;
    float band = exp(-pow((wl - uWave.z) / 46.0, 2.0));
    pos += wd / wl * band * uWave.w * 26.0 * uMotion;
  }

  vec2 clip = (pos / uRes) * 2.0 - 1.0;
  gl_Position = vec4(clip.x, -clip.y, 0.0, 1.0);

  // dust is fainter and a touch smaller; fade dust near the top/bottom edges (wrap seam)
  float edgeFade = smoothstep(-24.0, 36.0, pos.y) * (1.0 - smoothstep(uRes.y - 36.0, uRes.y + 24.0, pos.y));
  // shape particles: a little larger and denser than dust so the shape reads as one stippled form
  float vis = mix(uDustAlpha * mix(edgeFade, 1.0, shapeMix), uShapeAlpha, shapeMix);
  float size = aSize * mix(0.8, uShapeSize, shapeMix);
  // inside the stage a particle hangs in the air at some depth of the light sheet and is seen only where
  // the beam lights it there (Tyndall): warm, or the colour of the split light; outside nothing changes
  float inStage = stagePool(pos) * uStageInfo.y;
  vec3 lt = vec3(0.0);
  if (inStage > 0.0) {
    vec3 rd = viewRay(pos);
    float zp = (aSeed.z - 0.23) / 0.15 * uBeam2.z;
    lt = lightAt3(uCamPos + rd * ((zp - uCamPos.z) / min(rd.z, -0.05)));
  }
  float lmx = max(lt.r, max(lt.g, lt.b));
  float lit = clamp(lmx * 2.8, 0.0, 1.0);
  float lsat = lmx > 0.002 ? 1.0 - min(lt.r, min(lt.g, lt.b)) / lmx : 0.0;
  vec3 glint = mix(vec3(1.0, 0.96, 0.88), lmx > 0.002 ? lt / lmx : vec3(1.0), clamp(lsat * 1.3, 0.0, 0.55));
  vec3 rgb = mix(aColor.rgb, mix(vec3(0.36, 0.36, 0.35), glint, lit), inStage);
  float seen = mix(1.0, 0.5 + 1.2 * lit, inStage);
  vSize = size * uDpr * mix(1.0, 0.8 + 0.7 * lit, inStage);
  gl_PointSize = vSize;
  vColor = vec4(rgb, min(0.56 + 0.3 * lit * inStage, aColor.a * vis * seen) * uAlpha);
}
`,yt=`
attribute vec2 position;
attribute vec2 uv;
varying vec2 vUv;
void main() {
  vUv = uv;
  gl_Position = vec4(position, 0.0, 1.0);
}
`,bt=`
attribute vec2 position;   // viewBox units (y down)
attribute vec3 color;      // linear light
uniform vec4 uMap;         // the light map's rect (world xy: left, bottom, width, height)
uniform vec2 uOrigin;      // world x = viewBox x − uOrigin.x, world y = uOrigin.y − viewBox y
varying vec3 vCol;
void main() {
  vCol = color;
  vec2 w = vec2(position.x - uOrigin.x, uOrigin.y - position.y);
  gl_Position = vec4((w - uMap.xy) / uMap.zw * 2.0 - 1.0, 0.0, 1.0);
}
`,xt=`
uniform float uBeamGain;
varying vec3 vCol;
void main() {
  gl_FragColor = vec4(vCol * uBeamGain, 0.0);
}
`,St=`
uniform vec4 uSlab2;      // the podium's corner radius, the page's plane below it (y), the glass's height and half width
uniform float uHazeT;      // haze drift clock (s)
varying vec2 vUv;

const vec3 PAPER = vec3(0.980, 0.976, 0.961);
const vec3 WARM = vec3(1.0, 0.95, 0.86);
// the studio's soft key light (upper left, in front) shapes the things; the beam is the light that matters
const vec3 KEY = vec3(-0.329, 0.789, 0.519);

float hash(vec2 p) { return fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453); }
float vnoise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), f.x), mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), f.x), f.y);
}
float haze2(vec2 p) { return 0.62 * vnoise(p) + 0.38 * vnoise(p * 2.03 + vec2(17.1, 9.7)); }
float keyL(vec3 n) { return 0.74 + 0.26 * max(dot(n, KEY), 0.0) + 0.04 * n.y; }
const vec3 LUM = vec3(0.2126, 0.7152, 0.0722);
// the light as a camera with a vivid profile records it: white stays white, the split light more saturated
vec3 vivid(vec3 L) { return max(mix(vec3(dot(L, LUM)), L, 2.0), 0.0); }

/* ---- the glass: the outline extruded to its depth, bevelled round both faces, the folds grooved ---- */
float glassDist(vec3 p) {
  vec4 s = sdfAt(vec2(p.x, p.y - uGlassZ.w));
  float dz = abs(p.z) - uGlassZ.x;
  float d = min(max(s.x, dz), 0.0) + length(max(vec2(s.x, dz), 0.0));
  d = max(d, (s.x + dz + uGlassZ.y) * 0.7071068);
  float k = uSdf.w / uSdf.z;
  return max(d, (dz + uSdf.w - s.y * k) * inversesqrt(1.0 + k * k));
}
// for marching: away from the glass the distance to its box is enough (a lower bound, no texture read)
float glassD(vec3 p) {
  vec3 bq = abs(p - vec3(0.0, uGlassZ.w + uSlab2.z * 0.5, 0.0)) - vec3(uSlab2.w, uSlab2.z * 0.5, uGlassZ.x);
  float bd = length(max(bq, 0.0));
  return bd > 12.0 ? bd : glassDist(p);
}
vec3 glassN(vec3 p) {
  vec2 xy = vec2(p.x, p.y - uGlassZ.w);
  vec4 s = sdfAt(xy);
  float sz = p.z >= 0.0 ? 1.0 : -1.0;
  float dz = abs(p.z) - uGlassZ.x;
  float k = uSdf.w / uSdf.z;
  float cham = (s.x + dz + uGlassZ.y) * 0.7071068;
  float groove = (dz + uSdf.w - s.y * k) * inversesqrt(1.0 + k * k);
  vec2 side = normalize(s.zw + vec2(1e-5, 0.0));
  // the face it lies on: the front / back, a side wall, the bevel, a groove's wall
  vec3 n = vec3(0.0, 0.0, sz);
  float best = dz;
  if (s.x > best) {
    best = s.x;
    n = vec3(side, 0.0);
  }
  if (cham > best) {
    best = cham;
    n = normalize(vec3(side, sz));
  }
  if (groove > best) {
    const float e = 2.5;
    vec2 g = vec2(sdfAt(xy + vec2(e, 0.0)).y - sdfAt(xy - vec2(e, 0.0)).y, sdfAt(xy + vec2(0.0, e)).y - sdfAt(xy - vec2(0.0, e)).y) / (2.0 * e);
    n = normalize(vec3(-g * k, sz));
  }
  return n;
}

/* ---- the podium: a slab rounded into a disc (its corners as round as it is wide), its edges eased ---- */
float slabRect(vec2 xz) {
  vec2 c = vec2(0.0, (uSlab.y + uSlab.z) * 0.5);
  vec2 q = abs(xz - c) - vec2(uSlab.x, (uSlab.z - uSlab.y) * 0.5) + uSlab2.x;
  return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - uSlab2.x;
}
float slabD(vec3 p) {
  const float e = 9.0;
  vec2 w = vec2(slabRect(p.xz) + e, abs(p.y + uSlab.w * 0.5) - uSlab.w * 0.5 + e);
  return min(max(w.x, w.y), 0.0) + length(max(w, 0.0)) - e;
}
vec3 slabN(vec3 p) {
  vec2 h = vec2(0.6, 0.0);
  return normalize(vec3(slabD(p + h.xyy) - slabD(p - h.xyy), slabD(p + h.yxy) - slabD(p - h.yxy), slabD(p + h.yyx) - slabD(p - h.yyx)));
}

vec2 boxHit(vec3 ro, vec3 rd, vec3 b0, vec3 b1) {
  vec3 r = vec3(abs(rd.x) < 1e-5 ? 1e-5 : rd.x, abs(rd.y) < 1e-5 ? 1e-5 : rd.y, abs(rd.z) < 1e-5 ? 1e-5 : rd.z);
  vec3 t0 = (b0 - ro) / r;
  vec3 t1 = (b1 - ro) / r;
  vec3 lo = min(t0, t1);
  vec3 hi = max(t0, t1);
  return vec2(max(max(lo.x, lo.y), lo.z), min(min(hi.x, hi.y), hi.z));
}

// the light that crosses the glass, inside it (the traced map alone)
vec3 tracedAt(vec2 xy) { return texture2D(tLight, (xy - uMap.xy) / uMap.zw).rgb * uGlassZ.z * uLightAlpha; }

// the page below the podium (the plane y = uSlab2.y): the podium's soft shadow on it
vec3 backdrop(vec3 ro, vec3 rd) {
  float sh = 0.0;
  if (rd.y < -1e-4) {
    vec3 q = ro + rd * ((uSlab2.y - ro.y) / rd.y);
    sh = 0.15 * (1.0 - smoothstep(-0.3 * uSlab2.z, 0.4 * uSlab2.z, slabRect(q.xz)));
  }
  return PAPER * (1.0 - sh);
}

/* the podium: a slab of frosted glass — cool milk, its top lit a little less by the studio than its faces
   (the beam is the light that matters there), a fine frosted grain, the light glowing softly in it */
vec3 shadeSlab(vec3 p, vec3 n, float ga, vec3 rd) {
  vec3 alb = vec3(0.935, 0.945, 0.955);
  float key = 0.6 + 0.4 * max(dot(n, KEY), 0.0);
  vec3 c = alb * (n.y > 0.5 ? 0.88 * key / 0.9156 : 0.06 + keyL(n));
  if (n.y > 0.5) {
    c *= 1.0 - 0.05 * clamp((uSlab.z - p.z) / (uSlab.z - uSlab.y), 0.0, 1.0);
    c *= 1.0 + (hash(floor(p.xz * 0.9)) - 0.5) * 0.028;
  } else {
    // its faces: lighter towards its top edge, and the thin bright line glass edges show
    c *= 0.95 + 0.05 * clamp((p.y + uSlab.w) / uSlab.w, 0.0, 1.0);
    c += 0.05 * exp(-(p.y + uSlab.w) / 6.0);
  }
  // the glass hanging above it: a soft shadow under it, a soft occlusion round it
  if (ga > 0.0) {
    float H = uSlab2.z;
    float dg = glassDist(p + n * 2.0);
    float under = exp(-pow(max(abs(p.x) - 0.45 * uSlab2.w, 0.0) / (0.28 * H), 2.0)) * exp(-pow(max(abs(p.z) - uGlassZ.x, 0.0) / (0.14 * H), 2.0));
    c *= 1.0 - ga * (0.12 * exp(-dg / (0.14 * H)) + 0.16 * under * exp(-uGlassZ.w / (0.5 * H)) * step(0.5, n.y));
  }
  // the beam lights it — the split light lands here in its own colours, a little spread in the frosted glass
  vec3 q = p + n * 1.5;
  vec3 L = 0.4 * lightAt3(q) + 0.15 * (lightAt3(q + vec3(14.0, 0.0, 0.0)) + lightAt3(q - vec3(14.0, 0.0, 0.0)) + lightAt3(q + vec3(0.0, 0.0, 14.0)) + lightAt3(q - vec3(0.0, 0.0, 14.0)));
  vec3 E = vivid(L) * max(dot(n, -vec3(uBeam.zw, 0.0)), 0.0);
  c *= 1.0 + E * 2.6;
  // glass: a soft sheen of the studio's light, brighter where it is seen at a slant (its edges, the reliefs' flanks)
  float F = 0.04 + 0.96 * pow(1.0 - max(dot(n, -rd), 0.0), 5.0);
  c += pow(max(dot(reflect(-KEY, n), -rd), 0.0), 10.0) * 0.1;
  // its rim (the rounded edge, the side) is polished: clear glass there, a faint sea-green, a bright line of
  // the studio's light
  float edge = (1.0 - smoothstep(0.85, 0.98, n.y)) * step(p.y, -0.5);
  c = mix(c, vec3(0.8, 0.88, 0.875) * (0.82 + 0.28 * max(dot(n, KEY), 0.0)), edge * 0.32) + edge * pow(max(dot(n, KEY), 0.0), 6.0) * 0.22;
  return mix(c, vec3(1.0), F * 0.5);
}

vec3 shadeGlass(vec3 p, vec3 n, vec3 rd, vec3 behind) {
  vec3 ld = vec3(uBeam.zw, 0.0);
  vec3 Lo = lightAt3(p + n * 3.0);
  vec3 Li = vivid(tracedAt(vec2(p.x, p.y - uGlassZ.w) - n.xy * 5.0));
  float cv = max(dot(n, -rd), 0.0);
  float F = 0.04 + 0.96 * pow(1.0 - cv, 5.0);
  // frosted: milk lit by the studio (its faces in clearly different light), a blurred hint of what lies behind
  float key = 0.7 + 0.25 * max(dot(n, KEY), 0.0) + 0.12 * n.y;
  vec3 c = mix(behind * 0.94, vec3(0.9, 0.915, 0.93) * key, 0.72);
  c += Lo * WARM * max(dot(n, -ld), 0.0) * 0.32;                   // the faces the beam meets
  c += Li * WARM * 0.34;                                            // the light crossing it, scattered inside: it glows
  c += Lo * pow(max(dot(reflect(ld, n), -rd), 0.0), 24.0) * 0.5;    // a face mirroring the beam to the eye
  vec3 r = reflect(rd, n);
  c = mix(c, vec3(mix(0.8, 1.0, smoothstep(-0.3, 0.5, r.y))), F * 0.7);
  // thick glass reads darker at its grazing faces (not the ones facing up: they mirror the bright room above)
  c *= 1.0 - 0.3 * pow(1.0 - cv, 2.5) * (1.0 - max(n.y, 0.0));
  return c;
}

// the beam in the air: the haze it crosses scatters a little of it (Tyndall) — integrated through the sheet
vec3 hazeAlong(vec3 ro, vec3 rd, float tEnd, vec2 p) {
  float zs = uBeam2.z * 1.18;
  float rz = abs(rd.z) < 1e-4 ? -1e-4 : rd.z;
  float ta = (zs - ro.z) / rz;
  float tb = (-zs - ro.z) / rz;
  float t0 = max(min(ta, tb), 0.0);
  float t1 = min(max(ta, tb), tEnd);
  if (t1 <= t0) return vec3(0.0);
  float dt = (t1 - t0) / 6.0;
  float j = hash(p);
  vec2 ac = vec2(-uBeam.w, uBeam.z);
  vec3 acc = vec3(0.0);
  for (int i = 0; i < 6; i++) {
    vec3 q = ro + rd * (t0 + (float(i) + j) * dt);
    vec3 L = lightAt3(q);
    if (L.r + L.g + L.b < 0.004) continue;
    vec2 hq = vec2(dot(q.xy, uBeam.zw) * 0.0013 - uHazeT * 0.035, dot(q.xy, ac) * 0.0105 + q.z * 0.003 + uHazeT * 0.012);
    acc += vivid(L) * (0.3 + 1.2 * haze2(hq));
  }
  return acc * dt / (2.0 * uBeam2.z);
}

void main() {
  vec2 p = vec2(vUv.x, 1.0 - vUv.y) * uRes;
  float sa = uStageInfo.y;
  if (sa < 0.002 || stageSd(p) > 0.0) {
    gl_FragColor = vec4(0.0);
    return;
  }
  float ga = uStageInfo.w;
  float H = uSlab2.z;
  vec3 ro = uCamPos;
  vec3 rd = viewRay(p);

  // march the scene's box: the podium and the glass on it
  vec2 tb = boxHit(ro, rd, vec3(-uSlab.x, -uSlab.w, uSlab.y), vec3(uSlab.x, uGlassZ.w + H + 2.0, uSlab.z));
  float t = max(tb.x, 0.0);
  int id = 0;
  float near = 1e5;   // how close the ray passes the glass (its silhouette gets a dark refraction rim)
  if (tb.y > t) {
    for (int i = 0; i < 96; i++) {
      vec3 q = ro + rd * t;
      float dg = ga > 0.0 ? glassD(q) : 1e5;
      float ds = slabD(q);
      float d = min(dg, ds);
      near = min(near, dg);
      if (d < 0.25) {
        id = dg < ds ? 2 : 1;
        break;
      }
      t += d;
      if (t > tb.y) break;
    }
  }

  vec3 col;
  float hit = 1e9;
  if (id == 0) {
    // the studio: its air a shade darker than the page (so the light can be seen in it), fading into the
    // page towards the stage's edges (stagePool)
    col = backdrop(ro, rd) * 0.9;
  } else {
    vec3 q = ro + rd * t;
    hit = t;
    if (id == 1) {
      col = shadeSlab(q, slabN(q), ga, rd);
    } else {
      // what lies behind it, as the frosted glass blurs it: the page, and the podium's top softly where it is
      vec3 behind = backdrop(ro, rd) * 0.9;
      if (rd.y < 0.0) {
        float tp = -ro.y / rd.y;
        vec3 b = ro + rd * tp;
        if (tp > t) behind = mix(behind, vec3(0.935, 0.932, 0.922) * 0.87, 1.0 - smoothstep(-0.3 * H, 0.3 * H, slabRect(b.xz)));
      }
      col = mix(behind, shadeGlass(q, glassN(q), rd, behind), ga);
    }
  }
  // thick glass seen edge-on bends the room away: a thin dark rim round it
  if (id != 2) col *= 1.0 - 0.26 * ga * exp(-near / 2.5);
  col += hazeAlong(ro, rd, hit, p) * WARM * 0.58;
  // a soft shoulder for the brightest light — on its brightness, so the split light keeps its colour — then each
  // channel eased into 1
  float lum = max(dot(col, LUM), 1e-4);
  col *= 1.0 / (1.0 + max(lum - 0.86, 0.0) * 0.9);
  col = min(col, 0.9) + 0.1 * (1.0 - exp(-max(col - 0.9, 0.0) / 0.1));
  float A = sa * (id == 0 ? stagePool(p) : 1.0);
  // (the dither after the fade, or a faint stage bands while it comes in)
  gl_FragColor = vec4(clamp(col * A + (hash(p * 1.37 + 0.5) - 0.5) / 255.0, 0.0, A), A);
}
`,Ct=`
precision mediump float;
varying vec4 vColor;
varying float vSize;
void main() {
  vec2 c = gl_PointCoord - 0.5;
  float r = length(c);
  // anti-aliased disc, ~1 device px soft edge
  float aa = 1.0 / max(vSize, 1.0);
  float a = vColor.a * (1.0 - smoothstep(0.5 - aa, 0.5, r));
  if (a < 0.003) discard;
  gl_FragColor = vec4(vColor.rgb * a, a);
}
`,wt=`precision highp float;
uniform vec2 uRes;
`,Tt=`${wt}${_t}${gt}${vt}`,Et=`${wt}${yt}`,Dt=`${wt}${bt}`,Ot=`${wt}${xt}`,kt=`${wt}${gt}${St}`,At=Array.from({length:10},(e,t)=>410+t*280/9);function jt(e){let t=e/1e3;return 1.73897+.015944/(t*t)}var Mt=(e,t,n,r)=>{let i=(e-t)/(e<t?n:r);return Math.exp(-.5*i*i)};function Nt(e){return[1.056*Mt(e,599.8,37.9,31)+.362*Mt(e,442,16,26.7)-.065*Mt(e,501.1,20.4,26.2),.821*Mt(e,568.8,46.9,40.5)+.286*Mt(e,530.9,16.3,31.1),1.217*Mt(e,437,11.8,36)+.681*Mt(e,459,26,13.8)]}var Pt=(()=>{let e=At.map(e=>{let[t,n,r]=Nt(e);return[Math.max(0,3.2406*t-1.5372*n-.4986*r),Math.max(0,-.9689*t+1.8758*n+.0415*r),Math.max(0,.0557*t-.204*n+1.057*r)]}),t=e.reduce((e,t)=>[e[0]+t[0],e[1]+t[1],e[2]+t[2]],[0,0,0]);return e.map(e=>[e[0]/t[0],e[1]/t[1],e[2]/t[2]])})();function Ft(e){let t=[],n=null,r=0,i=0;for(let a of e.matchAll(/([MLHVZ])([^MLHVZ]*)/gi)){let e=a[1].toUpperCase(),o=(a[2]??``).trim().split(/[\s,]+|(?=-)/).filter(Boolean).map(Number);if(e===`M`)t.push(n=[...o]),r=o[o.length-2]??r,i=o[o.length-1]??i;else if(!n)continue;else if(e===`L`)n.push(...o),r=o[o.length-2]??r,i=o[o.length-1]??i;else if(e===`H`)for(let e of o)n.push(r=e,i);else if(e===`V`)for(let e of o)n.push(r,i=e)}return t.filter(e=>e.length>=6)}function It(e,t,n,r,i=[]){for(let a of e){let e=a.length/2;for(let o=0;o<e;o++){let s=(o+1)%e;i.push(a[o*2]*t+n,a[o*2+1]*t+r,a[s*2]*t+n,a[s*2+1]*t+r)}}return i}var Lt=.001,Rt=10;function zt(e,t,n,r,i,a){let o=e[t*4],s=e[t*4+1],c=e[t*4+2]-o,l=e[t*4+3]-s,u=i*l-a*c;if(u>-1e-12&&u<1e-12)return 1/0;let d=o-n,f=s-r,p=(d*l-f*c)/u;if(p<=Lt)return 1/0;let m=(d*a-f*i)/u;return m<0||m>1?1/0:p}function Bt(e,t,n,r,i,a,o,s,c){let l={pts:[n,r],dir:[],trans:[],sig:[],kind:[]},u=n,d=r,f=!1,p=!1,m=1,h=e.length/4;for(let n=0;n<=Rt;n++){let r=1/0,g=-1,_=-1;for(let t=0;t<h;t++){let n=zt(e,t,u,d,i,a);n<r&&(r=n,g=t)}for(let e=0;e<t.length/4;e++){let n=zt(t,e,u,d,i,a);n<r&&(r=n,g=-1,_=e)}if(l.dir.push(i,a),l.trans.push(m),l.kind.push(f?1:p?2:0),g<0&&_<0||n===Rt){l.pts.push(u+i*s,d+a*s);break}if(u+=i*r,d+=a*r,l.pts.push(u,d),_>=0){l.sig.push(-1-_);break}let v=e[g*4+2]-e[g*4],y=e[g*4+3]-e[g*4+1],b=Math.hypot(v,y)||1,x=y/b,S=-v/b,C=-(x*i+S*a);C<0&&(x=-x,S=-S,C=-C);let w=f?o:1,T=f?1:o,E=w/T,D=1-E*E*(1-C*C);if(D<0){if(c){l.sig.push(g*2+1);break}i+=2*C*x,a+=2*C*S,l.sig.push(g*2+1)}else{let e=Math.sqrt(D),t=((w-T)/(w+T))**2,n=w<=T?C:e;m*=1-(t+(1-t)*(1-n)**5),i=E*i+(E*C-e)*x,a=E*a+(E*C-e)*S,f&&(p=!0),f=!f,l.sig.push(g*2)}let O=Math.hypot(i,a)||1;i/=O,a/=O}return l}function Vt(e,t,n){let r=Math.max(4,Math.round(t.rays)),i=Math.cos(t.dir),a=Math.sin(t.dir),o=t.width/(r-1),s=e=>{let t=e/(r-1),n=(e,t)=>Math.min(1,Math.max(0,t/e)),i=n(.14,t),a=n(.14,1-t);return i*i*(3-2*i)*a*a*(3-2*a)},c=n?.pos.length?n.pos.length/2:0,l=n?.pos??new Float32Array,u=n?.col??new Float32Array,d=0,f=e=>{if(e<=c)return;c=Math.max(e,c*2,6e3);let t=new Float32Array(c*2),n=new Float32Array(c*3);t.set(l.subarray(0,d*2)),n.set(u.subarray(0,d*3)),l=t,u=n},p=(e,t,n,r,i)=>{l[d*2]=e,l[d*2+1]=t,u[d*3]=n,u[d*3+1]=r,u[d*3+2]=i,d++},m=t.frost??0,h=[0,.012,.035,.08,.16,.3,.55,1];return At.forEach((n,c)=>{let l=jt(n),[u,g,_]=Pt[c],v=null,y=0;for(let n=0;n<r;n++){let c=(n/(r-1)-.5)*t.width,b=Bt(e,t.stops??[],t.x-a*c,t.y+i*c,i,a,l,t.length,m>0),x=s(n);if(v){let e=v,n=b,r=Math.min(e.pts.length,n.pts.length)/2-1;f(d+r*6*h.length);for(let i=0;i<r&&!(i>0&&e.sig[i-1]!==n.sig[i-1]);i++){let r=i<e.sig.length,a=i<n.sig.length,s=r!==a||r&&e.sig[i]!==n.sig[i],c=e.dir[i*2],l=e.dir[i*2+1],d=(e.trans[i]+n.trans[i])*.5,f=(e,t,n,r)=>Math.min(3,o/Math.max(.3*o,Math.abs((n-e)*l-(r-t)*c))),v=e.pts[i*2],b=e.pts[i*2+1],S=e.pts[i*2+2],C=e.pts[i*2+3],w=n.pts[i*2],T=n.pts[i*2+1],E=n.pts[i*2+2],D=n.pts[i*2+3];if(r!==a){let e=Math.hypot(S-v,C-b),t=Math.hypot(E-w,D-T);r?(E=w+(E-w)*e/t,D=T+(D-T)*e/t):(S=v+(S-v)*t/e,C=b+(C-b)*t/e)}let O=d*t.intensity,k=y,A=x;if(m>0&&!s&&e.kind[i]===2&&i===e.pts.length/2-2&&i===n.pts.length/2-2){let e=w-v,t=T-b,n=Math.hypot(e,t)||1;e/=n,t/=n;let r=Math.tan(m/2),i=[v,b],a=[w,T],o=f(v,b,w,T)*O;for(let n=1;n<h.length;n++){let s=h[n],c=Math.hypot(S-v,C-b)*s,l=[v+(S-v)*s-e*c*r,b+(C-b)*s-t*c*r],d=[w+(E-w)*s+e*c*r,T+(D-T)*s+t*c*r],m=f(l[0],l[1],d[0],d[1])*O;p(i[0],i[1],u*o*k,g*o*k,_*o*k),p(l[0],l[1],u*m*k,g*m*k,_*m*k),p(d[0],d[1],u*m*A,g*m*A,_*m*A),p(i[0],i[1],u*o*k,g*o*k,_*o*k),p(d[0],d[1],u*m*A,g*m*A,_*m*A),p(a[0],a[1],u*o*A,g*o*A,_*o*A),i=l,a=d,o=m}continue}let j=f(v,b,w,T)*O,M=f(S,C,E,D)*O;if(p(v,b,u*j*k,g*j*k,_*j*k),p(S,C,u*M*k,g*M*k,_*M*k),p(E,D,u*M*A,g*M*A,_*M*A),p(v,b,u*j*k,g*j*k,_*j*k),p(E,D,u*M*A,g*M*A,_*M*A),p(w,T,u*j*A,g*j*A,_*j*A),s)break}}v=b,y=x}}),{pos:l,col:u,count:d}}var $=[205,262,846,740],Ht=Ft(i),Ut=Ht.flatMap(e=>e.filter((e,t)=>t%2==0)),Wt=Ht.flatMap(e=>e.filter((e,t)=>t%2==1)),Gt=(Math.min(...Ut)+Math.max(...Ut))/2,Kt=Math.max(...Wt),qt=Kt-Math.min(...Wt),Jt=(Math.max(...Ut)-Math.min(...Ut))/2,Yt=[578,271,488,435,585,558,703,708,460,697,382,569,819,584,706,433,703,708,794,831,660,993,572,863];function Xt(e,t){let n=!1;for(let r of Ht){let i=r.length/2;for(let a=0,o=i-1;a<i;o=a++){let i=r[a*2],s=r[a*2+1],c=r[o*2],l=r[o*2+1];s>t!=l>t&&e<(c-i)*(t-s)/(l-s)+i&&(n=!n)}}return n}var Zt=(e,t,n,r,i,a)=>{let o=i-n,s=a-r,c=Math.max(0,Math.min(1,((e-n)*o+(t-r)*s)/(o*o+s*s)));return Math.hypot(e-n-o*c,t-r-s*c)};function Qt(e=384){let t=[-Jt-56,qt+56,Jt*2+112,qt+112],n=e,r=Math.round(e*t[3]/t[2]),i=[];for(let e of Ht){let t=e.length/2;for(let n=0;n<t;n++){let r=e[n*2],a=e[n*2+1],o=e[(n+1)%t*2],s=e[(n+1)%t*2+1],c=Math.hypot(o-r,s-a)||1,l=(s-a)/c,u=-(o-r)/c;Xt((r+o)/2+l*2,(a+s)/2+u*2)&&(l=-l,u=-u),i.push(r,a,o,s,l,u)}}let a=Yt,o=new Uint8Array(n*r*4);for(let e=0;e<r;e++){let s=Kt-(t[1]-(e+.5)/r*t[3]);for(let r=0;r<n;r++){let c=t[0]+(r+.5)/n*t[2]+Gt,l=1/0,u=0;for(let e=0;e<i.length;e+=6){let t=Zt(c,s,i[e],i[e+1],i[e+2],i[e+3]);t<l&&(l=t,u=e)}let d=1/0;for(let e=0;e<a.length;e+=4)d=Math.min(d,Zt(c,s,a[e],a[e+1],a[e+2],a[e+3]));let f=Xt(c,s)?-l:l,p=(e*n+r)*4;o[p]=Math.round(Math.max(0,Math.min(1,f/56/2+.5))*255),o[p+1]=Math.round(Math.min(1,d/32)*255),o[p+2]=Math.round((.5+.5*i[u+4])*255),o[p+3]=Math.round((.5-.5*i[u+5])*255)}}return{data:o,w:n,h:r,box:t}}var $t=1.75,en=1.1,tn=1.7,nn=.6,rn=.16,an=48,on=1.6,sn=.35,cn=It(Ht,1,0,0),ln=48e3,un=e=>Math.max(0,Math.min(1,e)),dn=[61/255,61/255,58/255],fn=[135/255,134/255,127/255],pn=[217/255,119/255,87/255];function mn(e){let t=e>>>0;return()=>{t=t+1831565813>>>0;let e=t;return e=Math.imul(e^e>>>15,e|1),e^=e+Math.imul(e^e>>>7,e|61),((e^e>>>14)>>>0)/4294967296}}var hn=e=>e<.5?4*e*e*e:1-(-2*e+2)**3*.5;function gn(e,t){let n=0;for(let r=512;r>0;r>>=1){let i=+((e&r)>0),a=+((t&r)>0);if(n+=r*r*(3*i^a),a===0){i===1&&(e=1023-e,t=1023-t);let n=e;e=t,t=n}}return n}function _n(e,t,n,r,i){let a=new Float64Array(t),o=1023/i;for(let i=0;i<t;i++){let t=Math.max(0,Math.min(1023,Math.round((e[i*2]-n)*o))),s=Math.max(0,Math.min(1023,Math.round((e[i*2+1]-r)*o)));a[i]=gn(t,s)*32768+i}a.sort();let s=new Uint32Array(t);for(let e=0;e<t;e++)s[e]=a[e]%32768;return s}function vn(e){let{host:t}=e,n=Math.max(64,Math.min(32e3,Math.round(e.count))),i=document.createElement(`canvas`);i.setAttribute(`aria-hidden`,`true`);let a={alpha:!0,premultipliedAlpha:!0,antialias:!1,depth:!1,stencil:!1,powerPreference:`low-power`},o=null;try{o=i.getContext(`webgl2`,a)??i.getContext(`webgl`,a)}catch{o=null}if(!o)return null;let c;try{c=new ie({canvas:i,dpr:Math.min(window.devicePixelRatio||1,$t),alpha:!0,premultipliedAlpha:!0,antialias:!1,depth:!1,stencil:!1,powerPreference:`low-power`,webgl:2})}catch{return null}let l=c.gl;if(!l)return null;l.clearColor(0,0,0,0),t.appendChild(i);let u=mn(24301),d=new Float32Array(n*2),f=new Float32Array(n*2),p=new Float32Array(n*4),m=new Float32Array(n*4),h=new Float32Array(n*4),g=new Float32Array(n),_=Math.max(1,t.clientWidth||window.innerWidth),v=Math.max(1,t.clientHeight||window.innerHeight);for(let e=0;e<n;e++){let t=u(),n=dn,r=.25+u()*.3;t<.065?(n=pn,r=.5):t<.365&&(n=fn,r=.32+u()*.23),h.set([n[0],n[1],n[2],r],e*4),g[e]=1.2+1.8*u()**1.6,m[e*4]=(u()-.5)*2,m[e*4+1]=.25+u()*.75,m[e*4+2]=.08+u()*.3,m[e*4+3]=u()*10;let i=(u()+u()+u()-1.5)/1.5,a=(u()+u()+u()-1.5)/1.5;f[e*2]=_*.5+i*_*.62,f[e*2+1]=v*.46+a*v*.62,d[e*2]=f[e*2],d[e*2+1]=f[e*2+1],p.set([0,0,0,1],e*4)}let y=new ee(l,{aStart:{size:2,data:d,usage:l.DYNAMIC_DRAW},aTarget:{size:2,data:f,usage:l.DYNAMIC_DRAW},aMorph:{size:4,data:p,usage:l.DYNAMIC_DRAW},aSeed:{size:4,data:m},aColor:{size:4,data:h},aSize:{size:1,data:g}}),b={uRes:{value:[_,v]},uDpr:{value:c.dpr},uOffset:{value:[0,0]},uScrollDelta:{value:0},uTime:{value:1e4},uDur:{value:en},uClock:{value:0},uSpread:{value:2.5},uDustAmp:{value:26},uDustAlpha:{value:.5},uShapeAlpha:{value:1.3},uShapeSize:{value:1.4},uTurb:{value:0},uScrollVel:{value:0},uPointer:{value:[-1e4,-1e4,0]},uPointerR:{value:120},uWave:{value:[0,0,0,0]},uAlpha:{value:0},uMotion:{value:+!!e.motion},uLightAlpha:{value:0}},x=new ft(l,{width:2,height:2,depth:!1}),S=()=>[0,0,0],C={tLight:{value:x.texture},uLightAlpha:b.uLightAlpha,uStage:{value:[0,0,1,1]},uStageInfo:{value:[24,0,0,0]},uCamPos:{value:S()},uCamX:{value:S()},uCamY:{value:S()},uCamZ:{value:S()},uCam:{value:[0,0,1]},uMap:{value:[0,0,1,1]},uBeam:{value:[0,0,1,0]},uBeam2:{value:[1,0,1,0]},uGlassZ:{value:[1,1,1/sn,0]},uSlab:{value:[1,-1,1,1]},tSdf:{value:new dt(l,{image:new Uint8Array([255,255,128,128]),width:1,height:1,generateMipmaps:!1})},uSdfBox:{value:[0,1,1,1]},uSdf:{value:[56,32,13,5.5]}};Object.assign(b,C);let w={uSlab2:{value:[1,-1,qt,Jt]},uHazeT:{value:0}},T={uRes:b.uRes,uMap:C.uMap,uOrigin:{value:[Gt,Kt]},uBeamGain:{value:0}},E,D,O;try{E=new te(l,{vertex:Tt,fragment:Ct,uniforms:b,transparent:!0,depthTest:!1,depthWrite:!1}),D=new te(l,{vertex:Dt,fragment:Ot,uniforms:T,transparent:!0,depthTest:!1,depthWrite:!1,cullFace:!1}),O=new te(l,{vertex:Et,fragment:kt,uniforms:{uRes:b.uRes,...C,...w},depthTest:!1,depthWrite:!1})}catch{return i.remove(),null}if([E,D,O].some(e=>!l.getProgramParameter(e.program,l.LINK_STATUS)))return i.remove(),null;E.setBlendFunc(l.ONE,l.ONE_MINUS_SRC_ALPHA),D.setBlendFunc(l.ONE,l.ONE);let k=new st(l,{mode:l.POINTS,geometry:y,program:E,frustumCulled:!1}),A=new st(l,{geometry:new pt(l),program:O,frustumCulled:!1}),j=ln,M=e=>new ee(l,{position:{size:2,data:new Float32Array(e*2),usage:l.DYNAMIC_DRAW},color:{size:3,data:new Float32Array(e*3),usage:l.DYNAMIC_DRAW}}),N=new st(l,{geometry:M(j),program:D,frustumCulled:!1});N.geometry.setDrawRange(0,0);let P=e.motion,F=null,I=0,L=en,R=0,z=window.scrollY,B=0,V=0,ne=window.scrollY,H=0,U=0,W=1,G=-1e4,K=-1e4,re=-1e4,ae=-1e4,oe=0,q=-1e4,J={x:0,y:0,at:-1e4,strength:0},se=0,ce=0,le=null,ue,de=``,fe=``,pe=!1,me=Q.yaw,he=Q.pitch,ge=!1,Y=!1,X=!1,_e=typeof document<`u`&&document.hidden,ve=0,ye=0,be=!1,xe=0,Se=0,Ce=0,we=0,Te=[],Ee=performance.now(),Z=()=>(performance.now()-Ee)/1e3,De=()=>{if(!F)return;let e=F.anchor.getBoundingClientRect();B=e.left,V=e.top},Oe=(e,t,n,r)=>{let i=d[e*2],a=d[e*2+1],o=f[e*2],s=f[e*2+1],c=Math.min(1,Math.max(0,(t-I-p[e*4+2])/Math.max(.001,L*p[e*4+3]))),l=P?hn(c):1,u=i+(o-i)*l,h=a+(s-a)*l,g=o-i,_=s-a,y=Math.hypot(g,_);if(y>.5&&P){let t=Math.sin(Math.PI*l)*Math.min(y,520)*rn*m[e*4];u+=-_/y*t,h+=g/y*t}let b=p[e*4],x=p[e*4+1];if(x>.5)n[r]=u+B,n[r+1]=h+V;else{let t=v+96,i=h-(P?(window.scrollY-z)*m[e*4+2]:0);n[r]=u,n[r+1]=((i+an)%t+t)%t-an}return b+(x-b)*l},ke=mn(53335);function Ae(e){let t=new Float32Array(e*2),n=_,r=v,i=Math.max(1,Math.round(Math.sqrt(e*n/r))),a=Math.max(1,Math.ceil(e/i)),o=new Uint32Array(i*a);for(let e=0;e<o.length;e++)o[e]=e;for(let e=o.length-1;e>0;e--){let t=Math.floor(ke()*(e+1)),n=o[e];o[e]=o[t],o[t]=n}for(let s=0;s<e;s++){let e=o[s%o.length],c=e%i,l=Math.floor(e/i);t[s*2]=(c+ke())/i*n,t[s*2+1]=(l+ke())/a*r}return t}function je(e,t={}){if(X)return;let r=performance.now(),i=Z();De();let a=new Float32Array(n*2),o=new Float32Array(n);for(let e=0;e<n;e++)o[e]=Oe(e,i,a,e*2);F=e,De();let s=e?.points??new Float32Array,c=Math.min(n,Math.floor(s.length/2)),l=Ae(n-c),m=new Float32Array(n*2);for(let e=0;e<c;e++)m[e*2]=s[e*2]+B,m[e*2+1]=s[e*2+1]+V;m.set(l,c*2);let h=1/0,g=1/0,_=-1/0,v=-1/0;for(let e of[a,m])for(let t=0;t<n;t++){let n=e[t*2],r=e[t*2+1];n<h&&(h=n),n>_&&(_=n),r<g&&(g=r),r>v&&(v=r)}let x=Math.max(1,_-h,v-g),S=_n(a,n,h,g,x),C=_n(m,n,h,g,x),w=!!t.assemble,T=!!t.quick,E=Math.max(.2,t.tempo??1),D=(w?.75:T?.12:.42)*E;L=(w?tn:T?nn:en)*E;let O=Math.cos(143*Math.PI/180),k=-Math.sin(143*Math.PI/180),A=1/0,j=-1/0;for(let e=0;e<n;e++){let t=-(m[e*2]*O+m[e*2+1]*k);t<A&&(A=t),t>j&&(j=t)}let M=Math.max(1,j-A);R=0;for(let e=0;e<n;e++){let n=S[e],r=C[e],i=r<c,s=m[r*2],l=m[r*2+1];i?(d[n*2]=a[n*2]-B,d[n*2+1]=a[n*2+1]-V,f[n*2]=s-B,f[n*2+1]=l-V):(d[n*2]=a[n*2],d[n*2+1]=a[n*2+1],f[n*2]=s,f[n*2+1]=l);let h=(-(s*O+l*k)-A)/M,g=t.instant?0:D*(i?.6*h+.4*u():u()),_=.8+u()*.4;p[n*4]=o[n],p[n*4+1]=+!!i,p[n*4+2]=g,p[n*4+3]=_,R=Math.max(R,g+L*_)}y.attributes.aStart.needsUpdate=!0,y.attributes.aTarget.needsUpdate=!0,y.attributes.aMorph.needsUpdate=!0,I=t.instant||!P?i-1e3:i,z=window.scrollY,b.uSpread.value=e?.spread??2.5,b.uDustAlpha.value=.5*(e?.dust??1),W=+!!e,(!P||t.instant)&&(U=W),we=performance.now()-r,P?Le():Be()}function Me(e){e.count>j&&(N.geometry.remove(),j=Math.ceil(e.count*1.5),N.geometry=M(j));let t=N.geometry.attributes.position,n=N.geometry.attributes.color;t.data=e.pos.subarray(0,e.count*2),n.data=e.col.subarray(0,e.count*3),t.needsUpdate=!0,n.needsUpdate=!0,N.geometry.setDrawRange(0,e.count)}function Ne(e,t){let n=P?e:0,i=e-I,a=P&&R>0&&i<R?Math.sin(Math.PI*Math.max(0,i/R)):0;se+=(a-se)*Math.min(1,t*5),se<.002&&(se=0),ce=P?Math.min(1,ce+t*.7):1,b.uLightAlpha.value=ce,w.uHazeT.value=n;let o=r(),s=o?.stage.getBoundingClientRect(),c=o?.glass.getBoundingClientRect(),u=!!s&&!!c&&s.width>1&&c.height>1&&s.bottom>0&&s.top<v&&s.right>0&&s.left<_,d=u?un(o.state.alpha):0,f=u?un(o.state.glass):0,p=u?un(o.state.view??1):1;if(o&&!le){let e=Qt(_<768?288:384);le=new dt(l,{image:e.data,width:e.w,height:e.h,generateMipmaps:!1,minFilter:l.LINEAR,magFilter:l.LINEAR,wrapS:l.CLAMP_TO_EDGE,wrapT:l.CLAMP_TO_EDGE,flipY:!1}),C.tSdf.value=le,C.uSdfBox.value=e.box}if(!o||!s||!c||!u||d<.003&&f<.003){C.uStageInfo.value=[24,0,0,0],T.uBeamGain.value=0,pe=fe!==``,fe=``;return}let m=qt,h=Math.min(c.width/$[2],c.height/$[3]),g=c.left+(c.width-$[2]*h)/2-$[0]*h,y=c.top+(c.height-$[3]*h)/2-$[1]*h,S=Q.depth*m/2,E=Q.lift*m,D=E+(P?Q.bob*m*Math.sin(n*.8)*p:0),O=g+Gt*h,k=y+(Kt-m/2)*h,A=Q.distance*m,j=P?ge?.3:1:0,M=ge&&P?Math.max(-1,Math.min(1,re/_*2-1)):0,N=ge&&P?Math.max(-1,Math.min(1,ae/v*2-1)):0,F=Q.yaw+Q.swingYaw*M+Q.swayYaw*Math.sin(n*.21)*j,L=Q.pitch-Q.swingPitch*N+Q.swayPitch*Math.sin(n*.17)*j,ee=P?Math.min(1,t*2.6):1;me+=(F-me)*ee,he+=(L-he)*ee;let z=ht([0,E+m/2,0],A+S,me*p,he*p,O,k,A*h),te=Math.max(Jt+.1*m,Math.min(s.width/2*.86,s.width/2-10)/h),B=Math.min(te,Q.podium*m),V=Q.podiumThick*m,ne=-V-.05*m,H=Q.bob*m+10,U=(s.left-O)/h-.3*m,W=(s.right-O)/h+.3*m,G=m/2+(k-s.top)/h+.5*m,K=Math.min(m/2-(s.bottom-k)/h,ne-E)-.3*m,ie=_<768?640:1024,oe=Math.max(64,Math.min(1024,Math.round(ie*(G-K)/(W-U)))),q=mt.dir,Y=Math.cos(q),X=Math.sin(q),_e=$[0]+mt.ax*$[2],ve=$[1]+mt.ay*$[3],ye=f>.5,be=`${Math.round(U)},${Math.round(W)},${Math.round(K)},${Math.round(G)},${ie},${ye},${Math.round(B)}`;if(be!==de){de=be;let e=Math.min((_e-(U+Gt))/Y,(ve-(Kt-G))/X)+60;x.setSize(ie,oe),ue=Vt(ye?cn:[],{x:_e-Y*e,y:ve-X*e,dir:q,width:mt.width*$[3],length:Math.hypot(W-U,G-K)*1.3,rays:_<768?48:72,intensity:1,frost:mt.frost,stops:[Gt-B,Kt+E+H,Gt+B,Kt+E+H]},ue),Me(ue)}let xe=e-J.at,Se=P&&xe<on?J.strength*(1-xe/on)*Math.exp(-(((Math.hypot(O-J.x,k-J.y)-xe*820)/260)**2)):0,Ce=d*(.93+.07*Math.sin(n*.6))*(1+.25*se+.5*Se);T.uBeamGain.value=sn*Ce,C.uStage.value=[s.left,s.top,s.width,s.height],C.uStageInfo.value=[Math.max(24,Math.min(180,Math.min(s.width,s.height)*.16)),d,0,f],C.uCamPos.value=z.pos,C.uCamX.value=z.x,C.uCamY.value=z.y,C.uCamZ.value=z.z,C.uCam.value=[z.cx,z.cy,z.f],C.uMap.value=[U,K,W-U,G-K],C.uBeam.value=[_e-Gt,Kt-ve,Y,-X],C.uBeam2.value=[mt.width*$[3],Ce,Q.sheet*m,Math.tan(mt.frost/2)],C.uGlassZ.value=[S,Q.chamfer*m,1/sn,D],C.uSlab.value=[B,-B,B,V],w.uSlab2.value=[B,ne,m,Jt];let we=`${be},${d.toFixed(3)},${f.toFixed(3)},${D.toFixed(2)},${z.pos.map(e=>e.toFixed(1)).join()},${O.toFixed(1)},${k.toFixed(1)}`;pe=we!==fe,fe=we}function Pe(t){let n=performance.now(),r=Math.min(.1,Math.max(.001,t-ve||1/60));ve=t,De();let i=window.scrollY,a=(i-ne)/r;ne=i,H+=(Math.max(-6e3,Math.min(6e3,a))-H)*Math.min(1,r*7),Math.abs(H)<.5&&(H=0),G+=(re-G)*Math.min(1,r*10),K+=(ae-K)*Math.min(1,r*10);let o=P?Math.max(0,1-(t-q)/1.4):0;oe+=(o-oe)*Math.min(1,r*6),U+=(W-U)*Math.min(1,r*(W>U?4:3)),Math.abs(W-U)<.002&&(U=W);let s=t-J.at;b.uRes.value=[_,v],b.uDpr.value=c.dpr,b.uOffset.value=[B,V],b.uScrollDelta.value=i-z,b.uTime.value=P?t-I:1e4,b.uDur.value=L,b.uClock.value=t,b.uTurb.value=Math.min(1.5,Math.abs(H)/1800),b.uScrollVel.value=P?H:0,b.uPointer.value=[G,K,oe],b.uPointerR.value=_<768?80:120,b.uWave.value=s<1.2&&P?[J.x,J.y,s*820,J.strength*(1-s/1.2)]:[0,0,0,0],b.uAlpha.value=U,b.uMotion.value=+!!P,Ne(t,r),T.uBeamGain.value>0&&c.render({scene:N,target:x}),c.render({scene:A}),c.render({scene:k,clear:!1}),ye=t,be||(be=!0,e.onFirstFrame?.()),xe++,t-Se>=1&&(Ce=xe/(t-Se),xe=0,Se=t),Te.push(performance.now()-n),Te.length>240&&Te.shift()}let Fe=e=>e-I<R+.05||Math.abs(H)>4||oe>.01||e-J.at<on||U!==W||se>0||ce<1||pe,Ie=()=>{if(X)return;let e=Z();!Fe(e)&&e-ye<1/30-.004||(Pe(e),!F&&U===0&&Re())};function Le(){Y||X||_e||!P||(Y=!0,ve=Z(),ne=window.scrollY,s.ticker.add(Ie))}function Re(){Y&&(Y=!1,s.ticker.remove(Ie))}let ze=0;function Be(){X||ze||(ze=requestAnimationFrame(()=>{ze=0,!X&&Pe(Z())}))}let Ve=()=>{P?(F||U>0)&&Le():Be()},He=e=>{e.pointerType!==`touch`&&P&&((q<0||Z()-q>1.5)&&(G=e.clientX,K=e.clientY),re=e.clientX,ae=e.clientY,q=Z(),ge=!0,F&&Le())},Ue=e=>{e.relatedTarget||(q=-1e4,ge=!1)},We=()=>{_e=document.hidden,_e?Re():P&&(F||U>0)&&Le()},Ge=t=>{t.preventDefault(),Re(),e.onLost?.()};window.addEventListener(`scroll`,Ve,{passive:!0}),window.addEventListener(`pointermove`,He,{passive:!0}),window.addEventListener(`pointerout`,Ue,{passive:!0}),document.addEventListener(`visibilitychange`,We),i.addEventListener(`webglcontextlost`,Ge);function Ke(){let e=Math.max(1,t.clientWidth||window.innerWidth),n=Math.max(1,t.clientHeight||window.innerHeight);c.dpr=Math.min(window.devicePixelRatio||1,$t),c.setSize(e,n),_=e,v=n,P?Le():Be()}return Ke(),{setScene:je,setMotion(e){e!==P&&(P=e,e?(ve=Z(),Le()):(Re(),U=W,Be()))},impulse(e,t,n=1){P&&(J={x:e,y:t,at:Z(),strength:Math.max(0,Math.min(1.5,n))},Le())},draw:Be,resize:Ke,stats:()=>({count:n,scene:F?.key??null,morphing:Z()-I<R,running:Y,fps:Math.round(Ce*10)/10,cpuMs:Te.slice(),retargetMs:Math.round(we*100)/100}),destroy(){X=!0,Re(),ze&&cancelAnimationFrame(ze),window.removeEventListener(`scroll`,Ve),window.removeEventListener(`pointermove`,He),window.removeEventListener(`pointerout`,Ue),document.removeEventListener(`visibilitychange`,We),i.removeEventListener(`webglcontextlost`,Ge),l.getExtension(`WEBGL_lose_context`)?.loseContext(),i.remove()}}}function yn(e){return e<=768?2500:e<1180?5400:9e3}var bn=e=>Math.min(1,Math.max(0,e));function xn(e,t){if(e.length!==t.length)return!1;for(let n=0;n<e.length;n++)if(Math.abs(e[n]-t[n])>.5)return!1;return!0}function Sn(r,i){let s=i.motion,l=window.innerWidth,u=window.innerHeight,d=yn(l),f=!1,p=vn({host:r,count:d,motion:s,onFirstFrame:()=>n(`on`),onLost:()=>{n(`off`),ne()}});if(!p)return null;let m=new Map,h=e=>`${e.rev}|${l}x${u}`,g=null,_=null,v=0,y=!1;async function b(e){let t=e.options(),n=t.anchor?.current??e.el,r=Math.round(d*bn(t.density??.86)),i={count:r,width:l,height:u,compact:l<=768,el:e.el,rect:e.el.getBoundingClientRect(),helpers:c},a=h(e),o;try{o=t.target(i)}catch{return null}let s=n.getBoundingClientRect(),f;try{f=await o}catch{return null}let p=Math.min(f.length-f.length%2,r*2),g=new Float32Array(p);for(let e=0;e<p;e+=2)g[e]=f[e]-s.left,g[e+1]=f[e+1]-s.top;let _={key:a,points:g,anchor:n};return m.set(e.id,_),_}async function x(e,t={}){let n=++v;if(g=e,a(e?.id??null),!e){_=null,p.setScene(null,{instant:!s});return}let r=m.get(e.id);if(r&&r.key===h(e)||(r=await b(e)),n!==v||f||!r)return;if(_&&_.entry===e&&_.anchor===r.anchor&&xn(_.points,r.points)){_.key=r.key;return}_={entry:e,anchor:r.anchor,points:r.points,key:r.key};let i=e.options();p.setScene({key:`${e.id}:${r.key}`,points:r.points,anchor:r.anchor,spread:i.spread??2.5,dust:bn(i.dust??1)},{assemble:!y&&s,instant:!s,quick:t.quick,tempo:i.tempo}),y=!0,I(e),V()}let S=new Set,C=new Map,w=()=>{let e=null,t=1/0;for(let n of S){let r=n.el.getBoundingClientRect().height;r<t&&(t=r,e=n)}return e},T=0,E=()=>{T||=requestAnimationFrame(()=>{if(T=0,f)return;let e=w();e!==g&&x(e)})},D=new IntersectionObserver(e=>{for(let t of e){let e=C.get(t.target);e&&(t.isIntersecting?S.add(e):S.delete(e))}E()},{rootMargin:`-49.5% 0px -49.5% 0px`,threshold:0}),O=e=>{C.set(e.el,e),D.observe(e.el)},k=e=>{C.get(e.el)===e&&(C.delete(e.el),D.unobserve(e.el)),S.delete(e),m.delete(e.id),g===e&&E()};o().forEach(O);let A=t(e=>{e.type===`add`?O(e.entry):e.type===`remove`?k(e.entry):(m.delete(e.entry.id),g===e.entry&&x(e.entry))}),j=0,M=()=>{window.clearTimeout(j),j=window.setTimeout(()=>{g&&!f&&(m.delete(g.id),x(g,{quick:!0}))},140)},N=[],P=new WeakMap,F=typeof ResizeObserver<`u`?new ResizeObserver(e=>{let t=!1;for(let n of e){let e=`${Math.round(n.contentRect.width)}x${Math.round(n.contentRect.height)}`;P.get(n.target)!==void 0&&P.get(n.target)!==e&&(t=!0),P.set(n.target,e)}t&&M()}):null;function I(e){if(!F)return;let t=e.options().anchor?.current??e.el,n=t===e.el?[e.el]:[e.el,t];n.length===N.length&&n.every((e,t)=>e===N[t])||(N.forEach(e=>F.unobserve(e)),N=n,n.forEach(e=>F.observe(e)))}let L=0,ee=()=>{window.clearTimeout(L),L=window.setTimeout(()=>{let e=window.innerWidth,t=window.innerHeight;p.resize(),!(e===l&&Math.abs(t-u)<120)&&(l=e,u=t,m.clear(),g&&x(g,{quick:!0}))},160)};window.addEventListener(`resize`,ee,{passive:!0});let R=()=>{m.clear(),g&&M()};document.fonts?.addEventListener?.(`loadingdone`,R);let z=0,te=typeof window.requestIdleCallback==`function`?e=>window.requestIdleCallback(e,{timeout:1500}):e=>window.setTimeout(e,200),B=e=>typeof window.cancelIdleCallback==`function`?window.cancelIdleCallback(e):window.clearTimeout(e);function V(){B(z),z=te(()=>{if(f)return;let e=o().filter(e=>{if(e===g)return!1;let t=m.get(e.id);if(t&&t.key===h(e))return!1;let n=e.el.getBoundingClientRect();return n.bottom>-1.5*u&&n.top<2.5*u})[0];e&&b(e).then(()=>V())})}e((e,t,n)=>p.impulse(e,t,n));function ne(){f||(f=!0,v++,A(),D.disconnect(),F?.disconnect(),cancelAnimationFrame(T),B(z),window.clearTimeout(L),window.clearTimeout(j),window.removeEventListener(`resize`,ee),document.fonts?.removeEventListener?.(`loadingdone`,R),e(null),a(null),p.destroy())}return{setMotion(e){e!==s&&(s=e,p.setMotion(e))},destroy:ne}}export{Sn as startField};