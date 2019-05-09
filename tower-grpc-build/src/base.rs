
// struct Get(GetBar);

// // (prost) struct GetBar;
// // (prost) struct Bar;

// trait Foo {
// 	type GetFuture: Future<Item=Bar, Error=Error>;
// 	fn get(req: GetBar) -> GetFuture
// }

// trait FooService: Service<Get>, Response=Bar, Error=Error> {}

// impl<F: Foo> Service<Get> for F {
// 	type Response = Bar;
// 	type Error = Error;
// 	type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;

// 	fn poll_ready(&mut self) -> Poll<(), Self::Error> {
// 	    Ok(Async::Ready(()))
// 	}

// 	fn call(&mut self, req: Get) -> Self::Future {
// 		self.get(req.0)
// 	}
// }

// impl<F: Foo> FooService {}

use codegen;
use comments_to_rustdoc;
use prost_build;
use super::ImportType;

/// Generates service code
pub struct ServiceGenerator;

impl ServiceGenerator {
    /// Generate the gRPC server code
    pub fn generate(&self,
                    service: &prost_build::Service,
                    scope: &mut codegen::Scope) {
        self.define(service, scope);
    }

    fn define(&self,
              service: &prost_build::Service,
              scope: &mut codegen::Scope) {
        // Create scope that contains the generated base traits.
        {
            self.define_service_trait(service, scope);
            self.define_server_struct(service, scope);
            self.define_blanket_impl(service, scope)l

            // Define request structs
            for method in &service.methods {
            	scope.new_struct(&method.name)
            	    .vis("pub")
            	    .derive("Debug")
            	    .derive("Clone")
            	    .tuple_field(&method.input_type)
            	    ;


                // methods.import_type(&method.input_type, 2);

                // if !method.server_streaming {
                //     methods.import_type(&method.output_type, 2);
                // }

                // self.define_service_method(service, method, methods);
            }
        }
    }

    fn define_service_trait(&self,
                            service: &prost_build::Service,
                            scope: &mut codegen::Scope)
    {
        let mut service_trait = codegen::Trait::new(&service.name);
        service_trait.vis("pub")
            .parent("Clone")
            .doc(&comments_to_rustdoc(&service.comments))
            ;

        let mut tower_service_trait = codegen::Trait::new(&format!("{}Service", service.name));
        tower_service_trait.vis("pub")
            .parent("Clone");


        for method in &service.methods {
            let name = &method.name;
            let upper_name = ::to_upper_camel(&method.proto_name);

            let tower_bound = format!(
            	"tower_service::Service<{}, Response={}, Error=failure::Error>",
            	method.name, method.output_type);
            tower_service_trait.bound(tower_bound);

            let future_bound;

            let output_type = ::unqualified(&method.output_type, &method.output_proto_type, 1);

            if method.server_streaming {
                let stream_name = format!("{}Stream", &upper_name);
                let stream_bound = format!(
                    "futures::Stream<Item = {}, Error = failure::Error>",
                    output_type);

                future_bound = format!(
                    "futures::Future<Item = Self::{}, Error = failure::Error>",
                    stream_name);

                service_trait.associated_type(&stream_name)
                    .bound(&stream_bound);
            } else {
                future_bound = format!(
                    "futures::Future<Item = {}, Error = failure::Error>",
                    output_type);
            }

            let future_name = format!("{}Future", &upper_name);

            service_trait.associated_type(&future_name)
                .bound(&future_bound)
                ;

            for &ty in [&method.input_type, &method.output_type].iter() {
                if ::should_import(ty) {
                    let (path, ty) = ::super_import(ty, 1);

                    scope.import(&path, &ty);
                }
            }

            let input_type = ::unqualified(&method.input_type, &method.input_proto_type, 1);

            let request_type = if method.client_streaming {
            	let stream_name = format!("{}Streaming", &upper_name);
            	let stream_bound = format!(
            	    "futures::Stream<Item = {}, Error = failure::Error>",
            	    ::unqualified(&method.input_type, &method.input_proto_type, 1));

            	future_bound = format!(
            	    "futures::Future<Item = Self::{}, Error = failure::Error>",
            	    stream_name);

            	service_trait.associated_type(&stream_name)
            	    .bound(&stream_bound);
                format!("Self::{}", stream_name)
            } else {
            	input_type
            };

            service_trait.new_fn(&name)
                .arg_mut_self()
                .arg("request", &request_type)
                .ret(&format!("Self::{}Future", &upper_name))
                .doc(&comments_to_rustdoc(&method.comments))
                ;
        }

        scope.push_trait(service_trait);
    }

    fn define_server_struct(&self,
                            service: &prost_build::Service,
                            scope: &mut codegen::Scope)
    {
        let name = format!("{}Server", service.name);
        let lower_name = ::lower_name(&service.name);

        scope.new_struct(&name)
            .vis("pub")
            .derive("Debug")
            .derive("Clone")
            .generic("T")
            .field(&lower_name, "T")
            ;

        scope.new_impl(&name)
            .generic("T")
            .target_generic("T")
            .bound("T", &service.name)
            .new_fn("new")
            .vis("pub")
            .arg(&lower_name, "T")
            .ret("Self")
            .line(format!("Self {{ {} }}", lower_name))
            ;

        // MakeService impl
        // {
        //     let imp = scope.new_impl(&name)
        //         .generic("T")
        //         .target_generic("T")
        //         .impl_trait("tower::Service<()>")
        //         .bound("T", &service.name)
        //         .associate_type("Response", "Self")
        //         .associate_type("Error", "grpc::Never")
        //         .associate_type("Future", "futures::FutureResult<Self::Response, Self::Error>")
        //         ;


        //     imp.new_fn("poll_ready")
        //         .arg_mut_self()
        //         .ret("futures::Poll<(), Self::Error>")
        //         .line("Ok(futures::Async::Ready(()))")
        //         ;

        //     imp.new_fn("call")
        //         .arg_mut_self()
        //         .arg("_target", "()")
        //         .ret("Self::Future")
        //         .line("futures::ok(self.clone())")
        //         ;
        // }
        // for method in &service.methods {
        // 	let imp = scope.new_impl(&name)
        // 	    .generic("T")
        // 	    .target_generic("T")
        // 	    .impl_trait(&format!("tower::Service<{}>", &method.name))
        // 	    .bound("T", &service.name)
        // 	    .associate_type("Response", method.input_type)
        // 	    .associate_type("Error", "grpc::Never")
        // 	    .associate_type("Future", "futures::FutureResult<Self::Response, Self::Error>")
        // 	    ;

        // }
    }

    fn define_blanket_impl(&self,
                            service: &prost_build::Service,
                            scope: &mut codegen::Scope)
    {
    	let server_name = format!("{}Server", service.name);
        for method in &service.methods {
    		let imp = scope.new_impl(&server_name)
    		    .generic("T")
    		    .target_generic("T")
    		    .impl_trait(&format!("tower::Service<{}>", &method.name))
    		    .bound("T", &service.name)
    		    .associate_type("Response", method.input_type)
    		    .associate_type("Error", "failure::Error")
    		    .associate_type("Future", "futures::FutureResult<Self::Response, Self::Error>")
    		    ;

		    imp.new_fn("poll_ready")
		        .arg_mut_self()
		        .ret("futures::Poll<(), Self::Error>")
		        .line("Ok(futures::Async::Ready(()))")
		        ;

		    imp.new_fn("call")
		        .arg_mut_self()
		        .arg("request", method.input_type)
		        .ret("Self::Future")
		        .line("self.get(req.0)")
		        ;
        }
    }
}

